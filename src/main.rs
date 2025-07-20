use std::{io::{Read, Write}, net::TcpListener};
mod parser;
use parser::Parser;
mod router;
use router::Router;

// use http_server::parser::p
fn main() {
    // remove all tcp stuff, hide it from user, allow only defined mehods, and give function to start server on specific port
    let stream = TcpListener::bind("127.0.0.1:4421").unwrap();

    let (mut socket, addr) = stream.accept().expect("accept failed");

    let mut buffer = [0u8; 1024];
    socket.read(&mut buffer).unwrap();


    let request_obj = Parser::parse(&buffer);

    let mut router = Router::new();

    router.add_method(String::from("GET"), String::from("/"), || {
    println!("Handler called!");
    });
    
    println!("req obj??{:?}", request_obj);
    router.use_router(&request_obj, &socket);
    
    
    println!("lets see request obj {:?}", request_obj);


    socket.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap()

    
}

