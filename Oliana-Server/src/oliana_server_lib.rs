#![allow(unused_imports, unused_variables, unused_mut)]

use futures::prelude::*;
use tarpc::{
    client, context,
    server::{self, Channel},
};

// This is the service definition. It looks a lot like a trait definition.
// It defines one RPC, hello, which takes one arg, name, and returns a String.
#[tarpc::service]
pub trait Oliana {
    /// Runs an LLM and returns immediately; callers should concatinate results of generate_text_next_token() until it returns None for the reply. Return is some diagnostic text from server.
    async fn generate_text_begin(system_prompt: String, user_prompt: String) -> String;
    /// Returns None when token generation is complete
    async fn generate_text_next_token() -> Option<String>;
}

// This is the type that implements the generated World trait. It is the business logic
// and is used to start the server.
// There will be one OlianaServer client for each TCP connection; a dis-connect and re-connect will allocate a new OlianaServer.
// Also for each message OlianaServer::clone() is called -_- necessitaging syncronization primitives
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct OlianaServer {
    pub client_socket: std::net::SocketAddr,

    #[serde(skip)]
    pub shareable_procs: Option<std::sync::Arc<std::sync::RwLock<oliana_lib::launchers::TrackedProcs>>>,

    #[serde(skip)]
    pub ai_workdir_images: String,
    #[serde(skip)]
    pub ai_workdir_text: String,

    pub text_input_nonce: std::sync::Arc<std::sync::RwLock<usize>>,
    pub generate_text_next_byte_i: std::sync::Arc<std::sync::RwLock<usize>>, // Keeps track of how far into the output .txt file we have read for streaming purposes
}

impl OlianaServer {
    pub fn new(client_socket: std::net::SocketAddr,
               shareable_procs: std::sync::Arc<std::sync::RwLock<oliana_lib::launchers::TrackedProcs>>,
               ai_workdir_images: &str,
               ai_workdir_text: &str
        ) -> Self {
        Self {
            client_socket: client_socket,

            shareable_procs: Some(shareable_procs),
            ai_workdir_images: ai_workdir_images.to_string(),
            ai_workdir_text: ai_workdir_text.to_string(),

            text_input_nonce: std::sync::Arc::new(std::sync::RwLock::new( 0 )),

            generate_text_next_byte_i: std::sync::Arc::new(std::sync::RwLock::new( 0 )),

        }
    }

    pub fn read_text_input_nonce(&self) -> usize {
        let mut ret_val: usize = 0;
        match self.text_input_nonce.read() {
            Ok(text_input_nonce_rg) => {
                ret_val = *text_input_nonce_rg;
            }
            Err(e) => {
                eprintln!("{}:{} {:?}", file!(), line!(), e);
            }
        }
        ret_val
    }

    pub async fn increment_to_next_free_text_input_nonce(&mut self) -> Result<usize, Box<dyn std::error::Error>> {
        while tokio::fs::try_exists( self.get_current_text_input_json_path() ).await? {
            if let Ok(ref mut text_input_nonce_wg) = self.text_input_nonce.write() {
                **text_input_nonce_wg += 1;
            }
        }
        Ok(self.read_text_input_nonce())
    }
    pub fn get_current_text_input_json_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.ai_workdir_text).join(format!("{}.json", self.read_text_input_nonce()))
    }
    pub fn get_current_text_output_txt_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.ai_workdir_text).join(format!("{}.txt", self.read_text_input_nonce()))
    }
    pub fn get_current_text_output_done_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.ai_workdir_text).join(format!("{}.done", self.read_text_input_nonce()))
    }

    pub fn read_generate_text_next_byte_i(&self) -> usize {
        let mut ret_val: usize = 0;
        match self.generate_text_next_byte_i.read() {
            Ok(generate_text_next_byte_i_rg) => {
                ret_val = *generate_text_next_byte_i_rg;
            }
            Err(e) => {
                eprintln!("{}:{} {:?}", file!(), line!(), e);
            }
        }
        ret_val
    }

}

// These methods are run in the context of the client connection, on the server.
impl Oliana for OlianaServer {
    async fn generate_text_begin(mut self, _: context::Context, system_prompt: String, user_prompt: String) -> String {

        if let Ok(ref mut generate_text_next_byte_i_wg) = self.generate_text_next_byte_i.write() {
            **generate_text_next_byte_i_wg = 0;
        }

        if let Err(e) = self.increment_to_next_free_text_input_nonce().await {
            eprintln!("[ increment_to_next_free_text_input_nonce ] {:?}", e);
            return format!("[ increment_to_next_free_text_input_nonce ] {:?}", e);
        }

        let input_data = serde_json::json!({
            "system_prompt": system_prompt,
            "user_prompt": user_prompt
        });
        let input_data_s = input_data.to_string();

        let current_text_input_json = self.get_current_text_input_json_path();

        let response_txt_file = self.get_current_text_output_txt_path();
        if response_txt_file.exists() {
            if let Err(e) = tokio::fs::remove_file(response_txt_file).await {
                eprintln!("[ tokio::fs::remove_file ] {:?}", e);
                return format!("[ tokio::fs::remove_file ] {:?}", e);
            }
        }

        if let Err(e) = tokio::fs::write(current_text_input_json, input_data_s.as_bytes()).await {
            eprintln!("[ tokio::fs::write ] {:?}", e);
            return format!("[ tokio::fs::write ] {:?}", e);
        }

        String::new()
    }

    async fn generate_text_next_token(mut self, _: context::Context) -> Option<String> {
        // Right now we just wait for get_current_text_output_txt_path() to be created + return one giant chunk, but eventually Oliana-Text should iteratively update the file
        // so we can poll & return a streamed response.
        let response_txt_file = self.get_current_text_output_txt_path();
        while ! response_txt_file.exists() {
            tokio::time::sleep( tokio::time::Duration::from_millis(100) ).await;
        }

        let response_done_file = self.get_current_text_output_done_path();

        // Wait until the file's size is > self.read_generate_text_next_byte_i()
        let mut remaining_polls_before_give_up: usize = 12 * 10; // 12 seconds worth at 10 polls/sec
        loop {
            let next_byte_i = self.read_generate_text_next_byte_i();
            if let Ok(file_bytes) = tokio::fs::read(&response_txt_file).await {
                if file_bytes.len() < next_byte_i {
                    return None; // Somehow the file was truncated! .len() should always grow; it is allowed to be == next_byte_i.
                }
                if let Ok(the_string) = std::str::from_utf8(&file_bytes[next_byte_i..]) {

                    // Update the index we know we have read to to file_bytes.len()
                    match self.generate_text_next_byte_i.write() {
                        Ok(mut generate_text_next_byte_i_wg) => {
                            *generate_text_next_byte_i_wg = file_bytes.len();
                        }
                        Err(e) => {
                            eprintln!("{}:{} {:?}", file!(), line!(), e);
                        }
                    }

                    // It's possible to read 0 new bytes, in which case we do NOT want to return empty string; instead we fall down to the `response_done_file.exists() || remaining_polls_before_give_up < 1` check below.
                    if the_string.len() > 0 {
                        return Some(the_string.to_string());
                    }
                }
            }
            if response_done_file.exists() || remaining_polls_before_give_up < 1 { // What we just read must be the remaining bytes, because .done is created AFTER a write to .txt
                break;
            }
            tokio::time::sleep( tokio::time::Duration::from_millis(100) ).await;
            remaining_polls_before_give_up -= 1;
        }
        return None;
    }

}




