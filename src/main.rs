extern crate mio;
use mio::*;
use mio::tcp::{TcpListener, TcpStream};
use std::io::Read;
use std::io::Write;
use std::process::{Child, Command};

const CLIENT: Token = mio::Token(0);
const SERVER: Token = mio::Token(1);

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

fn copy_data(from: &mut Agent, to: &mut Agent) {
    let mut buf: [u8; 1024] = [0; 1024];
    let mut b = &mut buf[..];
    let rv = from.socket.read(b);
    let size = match rv {
        Err(err) => {
            panic!("read failed");
        },
        Ok(size) => size
    };
    println!("Buf {} ", size);
    let mut b2 = &mut b[0..size];

    let rv = to.socket.write_all(b2);
    match rv {
        Err(err) => {
            panic!("read failed");
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

    poll.poll(&mut events, None).unwrap();
    loop {
        for event in events.iter() {
            match event.token() {
                CLIENT => {
                    println!("Client ready");
                    copy_data(client, server);
                },
                SERVER => {
                    println!("Server ready");
                    copy_data(server, client);                    
                },
                _ => unreachable!()
            }
        }
    }    
}

fn main() {
    let mut server = Agent::new(String::from("server"),
                                String::from("/Users/ekr/dev/boringssl//build/ssl/test/bssl_shim"),
                                vec![
                                    String::from("-server"),
                                    String::from("-key-file"),
                                    String::from("/Users/ekr/dev/boringssl/ssl/test/runner/server.pem"),
                                    String::from("-cert-file"),
                                    String::from("/Users/ekr/dev/boringssl/ssl/test/runner/server.pem")]).unwrap();
    let mut client = Agent::new(String::from("client"),
                                String::from("/Users/ekr/dev/boringssl//build/ssl/test/bssl_shim"),
                                vec![]).unwrap();
    shuttle(&mut client, &mut server);
}
