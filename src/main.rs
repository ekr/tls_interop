extern crate mio;
use mio::*;
use mio::tcp::{TcpListener, TcpStream};
use std::io::Read;
use std::process::{Child, Command};

const SERVER: Token = mio::Token(0);
const A1: Token = mio::Token(0);
const A2: Token = mio::Token(1);

struct Agent {
    name : String,
    path : String,
    args : Vec<String>,
    socket : TcpStream,
    child: Child
}

impl Agent {
    fn new(name: String, path: String, args: Vec<String>) -> Result<Agent, i32> {
        let addr = "127.0.0.1:13265".parse().unwrap();
        let listener = TcpListener::bind(&addr).unwrap();
        
        // Start the subprocess.
        let mut command = Command::new(path.clone());
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
                      PollOpt::edge()).unwrap();
        let mut events = Events::with_capacity(1024);

        poll.poll(&mut events, None).unwrap();
        for event in events.iter() {
            match event.token() {
                SERVER => {
                    let sock = listener.accept();

                    println!("Accepted");

                    return Ok(Agent {
                        name: name,
                        path: path,
                        args: args,
                        child: child,
                        socket: sock.unwrap().0,
                    })
                },
                _ => return Err(-1),
            }
        }

        unreachable!()
    }
}

fn copy_data(from: &Agent, to: &Agent) {
    let mut buf = [u8; 1024];
    let size = from.socket.read(buf);
}

fn shuttle(agents: Vec<Agent>) {
    // Listen for connect
    // Create an poll instance
    let poll = Poll::new().unwrap();        
    poll.register(&agents[0].socket, A1, Ready::readable(),
                  PollOpt::level()).unwrap();
    poll.register(&agents[1].socket, A1, Ready::readable(),
                  PollOpt::level()).unwrap();
    let mut events = Events::with_capacity(1024);

    poll.poll(&mut events, None).unwrap();
    loop {
        for event in events.iter() {
            match event.token() {
                A1 => {
                    println!("A0 ready");
                },
                A2 => {
                    println!("A1 ready");
                },
                _ => unreachable!()
            }
        }
    }    
}

fn main() {
    let mut agents: Vec<Agent> = Vec::new();
    
    let a1 = Agent::new(String::from("a1"),
                            String::from("/Users/ekr/dev/boringssl//build/ssl/test/bssl_shim"),
                            Vec::new());
    agents.push(a1.unwrap());
    
    let a2 = Agent::new(String::from("a2"),
                            String::from("/Users/ekr/dev/boringssl//build/ssl/test/bssl_shim"),
                            vec![String::from("-server"),
                                 String::from("-key-file"),
                                 String::from("/Users/ekr/dev/boringssl/ssl/test/runner/server.pem"),
                                 String::from("-cert-file"),
                                 String::from("/Users/ekr/dev/boringssl/ssl/test/runner/server.pem")]
    );
    agents.push(a2.unwrap());    

    shuttle(agents);
}
