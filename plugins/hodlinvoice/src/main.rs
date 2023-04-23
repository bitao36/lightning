/* 
    This is a plugin which adds a "addholdinvoice" 
    command to Core Lightning. 
 */
#[macro_use]
extern crate serde_json;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use log::{debug, warn};

mod config;
mod model;

use model::{Preimagestate,hodlmethod,settlemethod,cancelmethod,listdatastore, listinvoices,getkeyfromstore,getdeltamethod,getblockheight,getblockheightmethod,PLUGIN_NAME,CLTV_HODL};
use config::{PluginState,read_config};

use cln_plugin::{Builder, Error, Plugin};

use anyhow::{anyhow};

use tokio;
use tokio::time;
/*  
The idea is that this plugin works very similar to how the LND addholdinvoce does.

So we have three  commands
    addholdinvoice <amount> <label> <description> <preimage>
    cancelinvoice <hash>
    settleinvoice <hash or preimage>
*/

#[tokio::main] 
async fn main() -> Result<(), anyhow::Error> {
    std::env::set_var("CLN_PLUGIN_LOG", "trace");

    
    let state = PluginState::new();   
    
    
    let plugin = Builder::new(tokio::io::stdin(), tokio::io::stdout())        
        .rpcmethod("addholdinvoice", 
        "Call this to create an invoice that will be held until released the preimage", 
        hodlmethod)
        .rpcmethod("settleinvoice", 
        "Call this to released the preimage", 
        settlemethod)
        .rpcmethod("cancelinvoice", 
        "Call this to cancel the invoice", 
        cancelmethod)
        .rpcmethod("getstatefromstore", 
        "Call this to get state from store", 
        getkeyfromstore)
        .rpcmethod("getdelta", 
        "Call this to get ctlv-delta value", 
        getdeltamethod)
        .rpcmethod("getblockheight", 
        "Call this to get ctlv-delta value", 
        getblockheightmethod)
        .hook("htlc_accepted", htlc_accept_handler)        
        .configure()        
        .await?
        .ok_or_else(|| anyhow!("Error configuring the plugin!"))?;
    
    

    read_config(&plugin, state.clone()).await?;

    plugin.start(state).await?.join().await?;

    Ok(())
}

/*  
 TO DO: what info do we need to pass into hodlinvoice?
 TO DO: what should we do with this information?
 nifty guesses: 
     - create an invoice, and remember the preimage/hash 
     
     question: What is the best way to remember the preimage?
               Saving the hash and the preimage in a hashmap maybe?

     Answer:Create an enum for the plugin state and
            store it in the datastore for persistence
            Preimagestate::Held      held when addholdinvoice command is used 
            Preimagestate::Released  released when settleinvoice command is used
            Preimagestate::Canceled  canceled with cancelinvoice command is used or autocancel

     - when an htlc with that same preimage/hash is 
     notified in htlc_accept_handler, hold the invoice!
     
     question: But I don't know what response I should return in the htcl_accept_handler method
        {"result": "?"}

     Answer: The handler must to do a loop until the state change with methods 
             settleinvoice , cancelinvoice or expiry. 
             if the command settleinvoice is used the response is
             {"result": "continue"}        
             if the command cancelinvoice is used the response is
             {"result": "fail"}        
          A way must be implemented that when the expiration time passes
          and none of the two commands already named have been used, detect this
          situation and acted as if they were using the cancelinvoice method.
          In conclusion, a self-cancellation when the invoice expires

     - when do we release the invoice?? 
      
      Answer: With a settlemethod 
*/

/// We get htlc and look for the hash.
/// if we have the hash stored with Preimagestate="held" 
/// We must do to a loop while the preimageState change

pub async fn htlc_accept_handler(plugin: Plugin<PluginState>,v: serde_json::Value,) -> Result<serde_json::Value, Error> 
{
    if let Some(htlc) = v.get("htlc") 
    {
        if let Some(pay_hash) = htlc.get("payment_hash").and_then(|pay_hash| pay_hash.as_str()) 
        {
            log::info!("pay_hash obtenido en htlc_accept_handler {}",pay_hash);   
            let rpc_path: PathBuf= Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file);
            let mut invoice = None;
            let cltv_expiry = match htlc.get("cltv_expiry") 
            {                
                Some(ce) => 
                {                
                    debug!("cltv_expiry en htlc = {}",ce);    
                    ce.as_u64().unwrap()
                }
                None => return Err(anyhow!("expiry not found! payment_hash: {}", pay_hash)),
            };
            loop {
                log::info!("Entrando al ciclo");   
                let preimagestate = match listdatastore(&rpc_path, Some(vec![PLUGIN_NAME.to_string(), pay_hash.to_string()])).await 
                {
                    Ok(resp) => {
                        if resp.datastore.len()== 1 
                        {
                            debug!("Obtuvimos un resultado de datastore y vamos a buscar el invoice de ese pay_hash");   

                            if invoice.is_none() 
                            {
                                invoice = Some(listinvoices(&rpc_path, None, Some(pay_hash.to_string()))
                                    .await?
                                    .invoices
                                    .first()
                                    .ok_or(anyhow!("invoice not found"))?
                                    .clone());
                            }
                            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                            debug!("unix epoch en segundos {}",now);   

                            if invoice.as_ref().unwrap().expires_at <= now 
                            {
                                warn!("hodling invoice with payment_hash: {} expired, rejecting!", pay_hash);
                                debug!("hodling invoice with payment_hash: {} expired, rejecting!", pay_hash);   
                                return Ok(json!({"result": "fail"}));
                            }
                            let cltv_delta = plugin.state().cltv_delta.lock().clone() as u64;
                            //let block_height=plugin.state().blockheight.lock().clone();
                            let block_height = getblockheight(&rpc_path).await.unwrap_or(0) as u64;
                            
                            debug!("cltv_delta= {} block_height = {}", cltv_delta,block_height);   

                            if cltv_expiry - cltv_delta <= block_height + CLTV_HODL as u64 
                            {
                                warn!("htlc timed out for payment_hash: {}, rejecting!", pay_hash);
                                debug!("htlc timed out for payment_hash: {}, rejecting!", pay_hash);
                                return Ok(json!({"result": "fail"}));
                            }
                            Preimagestate::from_str(resp.datastore.first().unwrap().string.as_ref().unwrap()).unwrap()

                        } 
                        else 
                        {
                            debug!("Payment hash={} Obtuvimos cero o mas de un resultado de datastore={:?}",pay_hash, resp.datastore);   
                            //return Err(anyhow!("wrong amount of results found for payment_hash: {} {:?}",
                              //  pay_hash, resp.datastore));
                            return Ok(json!({"result": "continue"}));    

                        }
                    },
                    Err(e) => 
                    {
                        debug!("payment_hash: {}  not our invoice: {}",  pay_hash,e.to_string());
                        //log::info!("payment_hash: {}  not our invoice: {}",  pay_hash,e.to_string());   
                        return Ok(json!({"result": "continue"}));
                    }
                };
                match preimagestate 
                {
                    Preimagestate::Held => {                        
                        debug!("Preimage held, hodling invoice with payment_hash: {}", pay_hash);                           
                    }
                    Preimagestate::Released => {                        
                        debug!("Preimage released, accepted invoice with payment_hash: {}", pay_hash);   
                        return Ok(json!({"result": "continue"}));
                    }
                    Preimagestate::Rejected => {                        
                        debug!("Preimage rejected, rejected invoice with payment_hash: {}", pay_hash);   
                        return Ok(json!({"result": "fail"}));
                    }
                }
                debug!("Sleeping 3 sgs");   
                time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
    Ok(json!({"result": "continue"}))
}