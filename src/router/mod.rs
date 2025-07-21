use std::{io::{self, Write}, net::TcpStream, thread::sleep, time::Duration};

use crate::parser::Request;

struct Route {
    method: String,
    path: String,
    callback: Box<dyn Fn(&Request, &mut Response)>,
}

pub struct Router {
     handlers: Vec<Route>,
}

pub struct Response<'a>{
    status: i32,
    text: String,
    socket: &'a mut TcpStream,
    protocol: String,
}

impl<'a> Response<'a>{
    pub fn new(socket: &'a mut TcpStream, protocol: String) -> Self {
        Response { status: 200, text: String::from("OK"), socket, protocol }
    }
    pub fn status(&mut self, status: i32) -> &mut Self {
        self.status = status;
        self
    }

    pub fn text(&mut self, text: String)  -> &mut Self{
        self.text = text;
        self
    }

    pub fn send(&mut self) {
        let response = format!("{} {} {}\r\n\r\n",self.protocol, self.status, self.text);
   
        self.socket.write_all(response.as_bytes()).unwrap();
    }
}


impl Router {
    pub fn new() -> Self {
        Router { handlers: Vec::new() }
    }
    pub fn add_method(&mut self, method: String, path: String, callback: fn(&Request, &mut Response)) {
        let route = Route{
            method,
            path,
            callback: Box::new(callback),
        };
        let _ = &self.handlers.push(route);
    }
    pub fn execute(&self, request: &Request, socket: &mut TcpStream) {
        for route in &self.handlers {
            if route.method == request.method && route.path == request.path {
                let mut response = Response::new( socket,request.version.clone());
                (route.callback)(request, &mut response);
                return;
            }
        }

        panic!("method not exists")
    }
}