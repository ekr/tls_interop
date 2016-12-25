extern crate clap;
#[macro_use]
extern crate log;
extern crate mio;
extern crate env_logger;
extern crate rustc_serialize;
use clap::{Arg, App};
use mio::*;
use mio::tcp::Shutdown;
use rustc_serialize::json;
use std::io::prelude::*;
use std::fs::File;
mod agent;
mod test_result;
use agent::Agent;
use test_result::TestResult;

const CLIENT: Token = mio::Token(0);
const SERVER: Token = mio::Token(1);

fn copy_data(poll: &Poll, from: &mut Agent, to: &mut Agent) {
    let mut buf: [u8; 16384] = [0; 16384];
    let mut b = &mut buf[..];
    let rv = from.socket.read(b);
    let size = match rv {
        Err(_) => {
            debug!("Error on {}", from.name);
            0
        },
        Ok(size) => size
    };
    if size == 0 {
        debug!("End of file on {}", from.name);
        poll.deregister(&from.socket).expect("Could not deregister socket");
        to.socket.shutdown(Shutdown::Write).expect("Shutdown failed");
        from.alive = false;
        return;
    }
    debug!("Buf {} ", size);

    let b2 = &b[0..size];
    let rv = to.socket.write_all(b2);
    match rv {
        Err(_) => {
            panic!("write failed");
        },
        _ => {
            debug!("Write succeeded");
        }
    };
}

fn shuttle(client: &mut Agent, server: &mut Agent) {
    // Listen for connect
    // Create an poll instance
    let poll = Poll::new().unwrap();
    poll.register(&client.socket, CLIENT, Ready::readable(),
                  PollOpt::edge()).unwrap();
    poll.register(&server.socket, SERVER, Ready::readable(),
                  PollOpt::edge()).unwrap();
    let mut events = Events::with_capacity(1024);

    while client.alive || server.alive {
        poll.poll(&mut events, None).unwrap();
        for event in events.iter() {
            match event.token() {
                CLIENT => {
                    debug!("Client ready");
                    copy_data(&poll, client, server);
                },
                SERVER => {
                    debug!("Server ready");
                    copy_data(&poll, server, client);
                },
                _ => unreachable!()
            }
        }
    }
}

#[derive(RustcDecodable, RustcEncodable)]
#[derive(Debug)]
struct TestCaseAgent {
    flags : Option<Vec<String>>,
}

#[derive(RustcDecodable, RustcEncodable)]
#[derive(Debug)]
struct TestCase {
    name: String,
    server_key : Option<String>,
    versions : Option<Vec<String>>,
    client : Option<TestCaseAgent>,
    server : Option<TestCaseAgent>,
}

#[derive(RustcDecodable, RustcEncodable)]
#[derive(Debug)]
struct TestCases {
    cases : Vec<TestCase>,
}

struct TestConfig {
    client_shim : String,
    server_shim : String,
    rootdir : String,
}

struct Results {
    ran : u32,
    succeeded: u32,
    failed: u32,
    skipped: u32,
}

impl Results {
    fn new() -> Results {
        Results {
            ran : 0,
            succeeded :0,
            failed : 0,
            skipped : 0,
        }
    }

    fn update(&mut self, case : &TestCase, result : TestResult) {
        self.ran += 1;
        match result {
            TestResult::OK => self.succeeded += 1,
            TestResult::Skipped => self.skipped += 1,
            TestResult::Failed => self.failed +=1
        }
    }
}
    
fn run_test_case(config: &TestConfig, case: &TestCase) -> TestResult {
    // Create the server args
    let mut server_args = vec![
        String::from("-server")
    ];
    let key_base =
        match case.server_key {
            None => String::from("rsa_1024"),
            Some(ref key) => key.clone()
        };
    server_args.push(String::from("-key-file"));
    server_args.push(config.rootdir.clone() + &key_base + &String::from("_key.pem"));
    server_args.push(String::from("-cert-file"));
    server_args.push(config.rootdir.clone() + &key_base + &String::from("_cert.pem"));
    server_args.push(String::from("-write-then-read"));

    if let Some(ref server) = case.server {
        match server.flags {
            None => (),
            Some (ref flags) => {
                for f in flags {
                    server_args.push(f.clone());
                }
            }
        };
    }

    let mut server = match Agent::new("server",
                                      &config.server_shim,
                                      server_args) {
        Ok(a) => a,
        Err(e) => { return TestResult::from_status(e); }
    };

    let mut client = match Agent::new("client",
                                      &config.client_shim,
                                      vec![]) {
        Ok(a) => a,
        Err(e) => { return TestResult::from_status(e); }
    };

    shuttle(&mut client, &mut server);

    return TestResult::merge(client.check_status(), server.check_status());
}


fn main() {
    env_logger::init().expect("Could not init logging");

    let matches = App::new("TLS interop tests")
        .version("0.0")
        .arg(Arg::with_name("client")
             .long("client")
             .help("The shim to use as the client")
             .takes_value(true)
             .required(true))
        .arg(Arg::with_name("server")
             .long("server")
             .help("The shim to use as the server")
             .takes_value(true)
             .required(true))
        .arg(Arg::with_name("rootdir")
             .long("rootdir")
             .help("The path where the working files are")
             .takes_value(true)
             .required(true))
        .arg(Arg::with_name("cases")
             .long("test-cases")
             .help("The test cases file to run")
             .takes_value(true)
             .required(true))
        .get_matches();

    let config = TestConfig {
        client_shim : String::from(matches.value_of("client").unwrap()),
        server_shim : String::from(matches.value_of("server").unwrap()),
        rootdir : String::from(matches.value_of("rootdir").unwrap()),
    };

    let mut f = File::open(matches.value_of("cases").unwrap()).unwrap();
    let mut s = String::from("");
    f.read_to_string(&mut s).expect("Could not read file to string");
    let cases : TestCases = json::decode(&s).unwrap();

    let mut results = Results::new();
    for c in cases.cases {
        let r = run_test_case(&config, &c);
        results.update(&c, r);
    }

    println!("Tests {}; Succeeded {}; Skipped {}, Failed {}",
             results.ran, results.succeeded, results.skipped, results.failed);

}
