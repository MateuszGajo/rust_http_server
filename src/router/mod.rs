use std::net::TcpStream;

use crate::parser::Request;

struct Route {
    method: String,
    path: String,
    callback: Box<dyn Fn()>,
}

pub struct Router {
     handlers: Vec<Route>,
}



impl Router {
    pub fn new() -> Self {
        Router { handlers: Vec::new() }
    }
    pub fn add_method(&mut self, method: String, path: String, callback: fn()) {
        let route = Route{
            method,
            path,
            callback: Box::new(callback),
        };
        &self.handlers.push(route);
    }
    pub fn use_router(&self, request: &Request, socket: &TcpStream) {
        for route in &self.handlers {
            if route.method == request.method && route.path == request.path {
                (route.callback)();
                return;
            }
        }

        panic!("method not exists")
    }
}