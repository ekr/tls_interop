#[macro_use]
extern crate log;
extern crate mio;
extern crate env_logger;
extern crate rustc_serialize;
use mio::*;
use mio::tcp::{TcpListener, TcpStream};
use rustc_serialize::json;
use std::io::prelude::*;
use std::fs::File;
use std::process::{Child, Command, ExitStatus};

const CLIENT: Token = mio::Token(0);
const SERVER: Token = mio::Token(1);

#[allow(dead_code)]
struct Agent {
    name : String,
    path : String,
    args : Vec<String>,
    socket : TcpStream,
    child: Child,
    alive : bool,
    exit_value : Option<ExitStatus>,
}

impl Agent {
    fn new(name: &String, path: &String, args: Vec<String>) -> Result<Agent, i32> {
        let addr = "127.0.0.1:13265".parse().unwrap();
        let listener = TcpListener::bind(&addr).unwrap();
        
        // Start the subprocess.
        let mut command = Command::new(path.clone());
        for arg in args.iter() {
            command.arg(arg);
        }
        
        command.arg("-port");
        command.arg(listener.local_addr().unwrap().port().to_string());
        command.arg("-exit-after-handshake");        

        let child = command.spawn().unwrap();

        // Listen for connect
        // Create an poll instance
        let poll = Poll::new().unwrap();        
        poll.register(&listener, SERVER, Ready::readable(),
                      PollOpt::edge()).unwrap();
        let mut events = Events::with_capacity(1024);

        poll.poll(&mut events, None).unwrap();
        for event in events.iter() {
            match event.token() {
                SERVER => {
                    let sock = listener.accept();

                    debug!("Accepted");

                    return Ok(Agent {
                        name: name.clone(),
                        path: path.clone(),
                        args: args,
                        socket: sock.unwrap().0,
                        child: child,
                        alive: true,
                        exit_value: None,
                    })
                },
                _ => return Err(-1),
            }
        }

        unreachable!()
    }

    fn check_status(&self) -> bool{
        match self.exit_value {
            None => unreachable!(),
            Some(ev) => {
                let code = ev.code().unwrap();
                debug!("Exit status for {} = {}", self.name, code);
                return code == 0
            }
        }
    }
}

fn copy_data(poll: &Poll, from: &mut Agent, to: &mut Agent) {
    let mut buf: [u8; 1024] = [0; 1024];
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
        from.alive = false;
        from.exit_value = Some(from.child.wait().unwrap());
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
    key_root : String,
}

fn run_test_case(config: &TestConfig, case: &TestCase) -> bool{
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
    server_args.push(config.key_root.clone() + &key_base + &String::from("_key.pem"));
    server_args.push(String::from("-cert-file"));
    server_args.push(config.key_root.clone() + &key_base + &String::from("_cert.pem"));
    
    let mut server = Agent::new(&String::from("server"),
                                &config.server_shim,
                                server_args).unwrap();
    let mut client = Agent::new(&String::from("client"),
                                &config.client_shim,
                                vec![]).unwrap();
    shuttle(&mut client, &mut server);

    if !(client.check_status() && server.check_status()) {
        info!("FAILED: {}", case.name);        
        return false;
    }
    true
}


fn main() {
    env_logger::init().expect("Could not init logging");
    
    let config = TestConfig {
        client_shim : String::from("/Users/ekr/dev/nss-dev/nss-sandbox2/dist/Darwin15.6.0_cc_64_DBG.OBJ/bin/nss_bogo_shim"),
        server_shim : String::from("/Users/ekr/dev/nss-dev/nss-sandbox2/dist/Darwin15.6.0_cc_64_DBG.OBJ/bin/nss_bogo_shim"),
        key_root : String::from("/Users/ekr/dev/boringssl/ssl/test/runner/"),
    };

    let mut f = File::open("cases.json").unwrap();
    let mut s = String::from("");
    f.read_to_string(&mut s).expect("Could not read file to string");
    let cases : TestCases = json::decode(&s).unwrap();

    let mut ran = 0;
    let mut succeeded = 0;
    let mut failed = 0;
    
    for c in cases.cases {
        ran += 1;
        if run_test_case(&config, &c) {
            succeeded += 1;
        } else {
            failed += 1;
        }
    }

    println!("Tests {}; Succeeded {}; Failed {}",
             ran, succeeded, failed);
}
