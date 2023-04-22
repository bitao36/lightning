use anyhow::{anyhow, Error};
use cln_plugin::ConfiguredPlugin;
use log::warn;
use parking_lot::Mutex;
use std::{path::Path, sync::Arc};

use tokio::fs;

#[derive(Clone)]
pub struct PluginState {
    pub cltv_delta: Arc<Mutex<u32>>,
    pub blockheight: Arc<Mutex<u64>>,
}
impl PluginState {
    pub fn new() -> PluginState {
        PluginState {
            cltv_delta: Arc::new(Mutex::new(42)),
            blockheight: Arc::new(Mutex::new(u64::default())),
        }
    }
    
}

pub async fn read_config(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
    state: PluginState,
) -> Result<(), Error> {
    let mut configfile = String::new();
    let dir = plugin.clone().configuration().lightning_dir;    

    log::info!("config dir= {}", dir);

    match fs::read_to_string(Path::new(&dir).parent().unwrap().join("config")).await 
    {
        Ok(file) => configfile = file,
        Err(_) => {
            match fs::read_to_string(Path::new(&dir).parent().unwrap().join("config")).await {
                Ok(file2) => configfile = file2,
                Err(_) => warn!("No config file found!"),
            }
        }
    }
    let mut cltv_delta = state.cltv_delta.lock();
    for line in configfile.lines() {
        let config_line: Vec<&str> = line.split('=').collect();
    
        if config_line.len() != 2 {
            continue;
        }
    
        let (name, value) = (config_line[0], config_line[1]);
    
        match name {
            "cltv-delta" => {
                if let Ok(n) = value.parse::<u32>() {
                    *cltv_delta = n ;
                    log::info!("clv-delta get from file: {}", n);
                } else {
                    return Err(anyhow!(
                        "Error: Could not parse a number from `{}` for {}",
                        value,
                        name,                        
                    ))
                }
            },
            _ => {}
        }
    }

    Ok(())
}