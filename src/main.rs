extern crate clap;
#[macro_use]
extern crate log;
extern crate mio;
extern crate env_logger;
extern crate rustc_serialize;
use clap::{Arg, App};
use mio::*;
use mio::channel::Receiver;
use mio::tcp::{TcpListener, TcpStream, Shutdown};
use rustc_serialize::json;
use std::io::prelude::*;
use std::fs::File;
use std::process::{Command, ExitStatus};
use std::thread;

const CLIENT: Token = mio::Token(0);
const SERVER: Token = mio::Token(1);
const FAILED: Token = mio::Token(2);

#[allow(dead_code)]
struct Agent {
    name : String,
    path : String,
    args : Vec<String>,
    socket : TcpStream,
    child: Receiver<i32>,
    alive : bool,
    exit_value : Option<ExitStatus>,
}

impl Agent {
    fn new(name: &str, path: &String, args: Vec<String>) -> Result<Agent, i32> {
        let addr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(&addr).unwrap();

        // Start the subprocess.
        let mut command = Command::new(path.to_owned());
        for arg in args.iter() {
            command.arg(arg);
        }

        command.arg("-port");
        command.arg(listener.local_addr().unwrap().port().to_string());

        let mut child = command.spawn().unwrap();

        // Listen for connect
        // Create an poll instance
        let poll = Poll::new().unwrap();
        poll.register(&listener, SERVER, Ready::readable(),
                      PollOpt::level()).unwrap();
        let mut events = Events::with_capacity(1024);

        // This is gross, but we can't reregister channels.
        // https://github.com/carllerche/mio/issues/506
        let (txf, rxf) = channel::channel::<i32>();
        let (txf2, rxf2) = channel::channel::<i32>();
        
        poll.register(&rxf, FAILED, Ready::readable(),
                      PollOpt::level()).unwrap();

        thread::spawn(move || {
            let ecode = child.wait().expect("failed waiting for subprocess");
            txf.send(match ecode.code() {
                None => -1,
                Some(e) => e
            });
            txf2.send(match ecode.code() {
                None => -1,
                Some(e) => e
            });
        });

        poll.poll(&mut events, None).unwrap();
        debug!("Poll finished!");
        for event in events.iter() {
            debug!("Event!");
            match event.token(){
                SERVER => {
                    let sock = listener.accept();

                    debug!("Accepted");
                    return Ok(Agent {
                        name: name.to_owned(),
                        path: path.to_owned(),
                        args: args,
                        socket: sock.unwrap().0,
                        child: rxf2,
                        alive: true,
                        exit_value: None,
                    })
                },
                FAILED => {
                    let err = rxf.try_recv().unwrap();
                    info!("Failed {}", err);
                    return Err(err);
                },
                _ => return Err(-1),
            }
        }
        
        debug!("Started {}", name);
        unreachable!()
    }

    // Read the status from the subthread.
    fn check_status(&self) -> TestResult {
        debug!("Getting status for {}", self.name);
        // try_recv() is nonblocking, so poll until it's readable.
        let poll = Poll::new().unwrap();
        poll.register(&self.child, mio::Token(0), Ready::readable(),
                      PollOpt::level()).unwrap();
        let mut events = Events::with_capacity(1);
        poll.poll(&mut events, None).unwrap();
        
        let code = self.child.try_recv().unwrap();
        debug!("Exit status for {} = {}", self.name, code);
        return TestResult::from_status(code);
    }
}

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

enum TestResult {
    OK,
    Skipped,
    Failed
}

impl TestResult {
    fn from_status(status: i32) -> TestResult {
        match status {
            0 => TestResult::OK,
            89 => TestResult::Skipped,
            _ => TestResult::Failed
        }
    }

    // Return a combined return status. If either side skipped, then
    // we mark it skipped. Otherwise we return OK only if both sides
    // reported OK.
    fn merge(a: TestResult, b: TestResult) -> TestResult{
        let res = (a, b);
        match res {
            (TestResult::Skipped, _) =>  TestResult::Skipped,
            (_, TestResult::Skipped) =>  TestResult::Skipped,
            (TestResult::Failed, _) =>  TestResult::Failed,
            (_, TestResult::Failed) =>  TestResult::Failed,
            (TestResult::OK, TestResult::OK) => TestResult::OK
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

    let mut ran = 0;
    let mut succeeded = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for c in cases.cases {
        ran += 1;
        match run_test_case(&config, &c) {
            TestResult::OK => succeeded += 1,
            TestResult::Skipped => skipped += 1,
            TestResult::Failed => failed +=1
        }
    }

    println!("Tests {}; Succeeded {}; Skipped {}, Failed {}",
             ran, succeeded, skipped, failed);
}
