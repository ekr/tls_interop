extern crate rustc_serialize;
extern crate mio;
use mio::*;
use mio::tcp::{TcpListener, TcpStream};
use rustc_serialize::json;
use std::env;
use std::io;
use std::io::prelude::*;
use std::fs::File;
use std::process::{Child, Command, ExitStatus};

const CLIENT: Token = mio::Token(0);
const SERVER: Token = mio::Token(1);

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

        let mut child = command.spawn().unwrap();

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

                    println!("Accepted");

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

    fn check_status(&self) {
        match self.exit_value {
            None => unreachable!(),
            Some(ev) => {
                println!("Exit status for {} = {}", self.name, ev.code().unwrap());
            }
        }
    }
}

fn copy_data(poll: &Poll, from: &mut Agent, to: &mut Agent) {
    let mut buf: [u8; 1024] = [0; 1024];
    let mut b = &mut buf[..];
    let rv = from.socket.read(b);
    let size = match rv {
        Err(err) => {
            println!("Error on {}", from.name);
            0
        },
        Ok(size) => size
    };
    if size == 0 {
        println!("End of file on {}", from.name);
        from.alive = false;
        from.exit_value = Some(from.child.wait().unwrap());
        return;
    }
    println!("Buf {} ", size);
    
    let mut b2 = &mut b[0..size];
    let rv = to.socket.write_all(b2);
    match rv {
        Err(err) => {
            panic!("write failed");
        },
        _ => {
            println!("Write succeeded");
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
                    println!("Client ready");
                    copy_data(&poll, client, server);
                },
                SERVER => {
                    println!("Server ready");
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
}

fn run_test_case(config: &TestConfig, case: &TestCase) {
    let mut server = Agent::new(&String::from("server"),
                                &config.server_shim,
                                vec![
                                    String::from("-server"),
                                    String::from("-key-file"),
                                    String::from("/Users/ekr/dev/boringssl/ssl/test/runner/rsa_1024_key.pem"),
                                    String::from("-cert-file"),
                                    String::from("/Users/ekr/dev/boringssl/ssl/test/runner/rsa_1024_cert.pem")]).unwrap();
    let mut client = Agent::new(&String::from("client"),
                                &config.client_shim,
                                vec![]).unwrap();
    shuttle(&mut client, &mut server);

    client.check_status();
    server.check_status();
}


fn main() {
    let config = TestConfig {
        client_shim : String::from("/Users/ekr/dev/nss-dev/nss-sandbox2/dist/Darwin15.6.0_cc_64_DBG.OBJ/bin/nss_bogo_shim"),
        server_shim : String::from("/Users/ekr/dev/nss-dev/nss-sandbox2/dist/Darwin15.6.0_cc_64_DBG.OBJ/bin/nss_bogo_shim"),
    };

    let mut f = File::open("cases.json").unwrap();
    let mut s = String::from("");
    f.read_to_string(&mut s).expect("Could not read file to string");
    let t : TestCases = json::decode(&s).unwrap();
}
