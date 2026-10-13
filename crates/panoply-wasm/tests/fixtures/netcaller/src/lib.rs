//! Tries to reach the network. Test fixture only.
wit_bindgen::generate!({ path: "wit", world: "module" });

struct Module;

impl Guest for Module {
    fn run(_inputs: Vec<InputFile>, _options: String) -> Result<Vec<Output>, String> {
        let r = std::net::TcpStream::connect("93.184.216.34:80");
        Ok(vec![Output::Note(format!("{:?}", r.is_ok()))])
    }
}

export!(Module);
