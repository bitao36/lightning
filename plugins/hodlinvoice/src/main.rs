/* 
    This is a plugin which adds a "addholdinvoice" 
    command to Core Lightning. 
 */
#[macro_use]
extern crate serde_json;

use bitcoin::hashes::HashEngine;
use bitcoin::hashes::Hash;
use bitcoin::consensus::encode::serialize_hex;

use rand::Rng;
use rand::thread_rng;
use cln_plugin::{Builder, Error, Plugin};
use std::{path::PathBuf,path::Path};
use anyhow::{anyhow};
//use cln_rpc::model::{responses,requests}; 
use cln_rpc::{
    model::*,
    primitives::{Amount,AmountOrAny,Sha256},
    ClnRpc,
};

use tokio;
#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let state = ();

/*  

The idea is that this plugin works very similar to how the LND addholdinvoce does.

So we have three  commands
    addholdinvoice <amount> <label> <description> <preimage>
    cancelinvoice <hash>
    settleinvoice <hash or preimage>
*/

    if let Some(plugin) = Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .rpcmethod("addholdinvoice", 
                   "Call this to create an invoice that will be held until released the preimage", 
                   hodlmethod)
        .rpcmethod("settleinvoice", 
                   "Call this to released the preimage", 
                   settlemethod)
        .rpcmethod("cancelinvoice", 
                   "Call this to cancel the invoice", 
                   cancelmethod)
        .hook("htlc_accepted", htlc_accept_handler)
        .start(state)
        .await?
    {
        plugin.join().await
    } else {
        Ok(())
    }
}


/*  
 TODO: what info do we need to pass into hodlinvoice?
 TODO: what should we do with this information?
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

/*  
example: addholdinvoice <amount> <label> <description> <preimage>

description is optional
preimage is optional

We create a preimage and with the parameters obtained
we use the cln_rpc library to create an invoice

         invoice <amount> <label> <expiry> <description> <preimage>

         expiry by default 86400 (24 hours) 

We must save the invoice <hash> and its state in the datastore to know
if the preimage is held or not
  
*/

async fn hodlmethod(_p: Plugin<()>, _v: serde_json::Value) -> Result<serde_json::Value, Error> {
    
    log::info!("Parametros obtenidos de holdinvoice  {}, plugin config= {}", _v,&_p.configuration().rpc_file);

    if let Some(arr) = _v.as_array() 
    {        
        
        if arr.len()>=2
        {
                //let amount_msat;
                //if _v[0].is_number()
               // {
                    let amount_msat_wrap=_v[0].as_u64().unwrap();
                    let amount_msat=Amount::from_msat(amount_msat_wrap);     
                //}

                let _label=  _v[1].to_string();
                let _expiry: u64=3600;                
                let _description=_v[2].to_string();

                let (pi,hash)= get_preimage_and_hash();                                  

                log::info!("Hash={} Preimage={}", hash,pi);

                let rpc_path: PathBuf= Path::new(&_p.configuration().lightning_dir).join(_p.configuration().rpc_file);
                let mut rpc = ClnRpc::new(rpc_path).await?;
               
                let invoice_request = rpc
                    .call(Request::Invoice(InvoiceRequest {
                    amount_msat: AmountOrAny::Amount(amount_msat),
                    description: _description,
                    label:_label,
                    expiry: Some(_expiry),
                    fallbacks: None,
                    preimage: Some(pi),
                    exposeprivatechannels: None,
                    cltv: Some(50),
                    deschashonly: None,
                }))
                .await
                .map_err(|e| anyhow!("Error calling invoice: {:?}", e))?;
                
                //log::info!("Hash de la preimage {}",hash.to_string());

                //We must get hash and stored it with Preimagestate "held"

                Ok(json!(invoice_request))
                
        }
        else 
        {            
            Ok(json!("Faltan parametros"))            
        }
         
    }
    else 
    {
        //log::info!("El valor no es un array");
        Ok(json!("No ingresaste parametros"))
    }   

    /* Validate parameters */

    //let _hash: i64 = _v[0].as_i64().unwrap(); 
    //let _amount: i64 = _v[1].as_i64().unwrap();
    //let _label = _v[2].as_str().unwrap();    
    
    /* Create preimage */
     
    /* Save invoice hash  and preimage */
        
    
}


/// Example cancelinvoice <hash>
/// The invoice must be searched with the hash
/// and Preimagestate must change to "canceled"


async fn cancelmethod(_p: Plugin<()>, _v: serde_json::Value) -> Result<serde_json::Value, Error> 
{
    let _hash=  _v[0].to_string();
    //1. Look for invoice hash
    //2. Change Preimagestate to "canceled"             
    //Ok(json!("htlc must fail and delete the stored preimage!"))
    //Ok(json!({"result": "fail","failure_message": "2002"}))
    Ok(json!({"result": "success"}))
}


/// example: settleinvoice <hash or preimage>
/// The invoice must be searched with the hash or preimage
/// and Preimagestate must change to "released"

async fn settlemethod(_p: Plugin<()>, _v: serde_json::Value) -> Result<serde_json::Value, Error> {
    
    //1. Look for invoice hash
    //2. Change Preimagestate to "released"             
    let _preimage=  _v[0].to_string();
    //Ok(json!("preimage must be released"))
    //Ok(json!({"result": "resolve","payment_key": preimage }))
    Ok(json!({"result": "success"}))
}

/// We get htlc and look for the hash.
/// if we have the hash stored with Preimagestate="held" 
/// We must do to a loop while the preimageState change
async fn htlc_accept_handler(_p: Plugin<()>,v: serde_json::Value,) -> Result<serde_json::Value, Error> {
    log::info!("Got a htlc accepted call: {}", v);

    if let Some(htlc) = v.get("htlc") 
    {
        //We get the hash 
        if let Some(pay_hash) = htlc
            .get("payment_hash")
            .and_then(|pay_hash| pay_hash.as_str())
        {
            log::info!("pay_hash: {}", pay_hash.to_string());
            //We must look for this hash to see if we have it stored
            //we make it a loop to held the preimage
            //which we will exit only when the Preimagestate is changed using
            //the commands settleinvoice, cancelinvoice or until it happens
            // the expiration time.

            /*             
            loop 
            {
                {
                    match preimagestate 
                    {
                        Preimagestate::Held => {
                            debug!("invoice with preimage held payment_hash: {}", pay_hash);
                        }
                        Preimagestate::Released => {
                            debug!("invoice with preimage released payment_hash: {}", pay_hash);
                            return Ok(json!({"result": "continue"}));
                        }
                        Preimagestate::Canceled => {
                            debug!("invoice canceled with payment_hash: {}", pay_hash);
                            return Ok(json!({"result": "fail"}));
                        }
                    }
                }
                time::sleep(Duration::from_secs(3)).await;
            }
            */

        }
    }
        
    Ok(json!({"result": "continue"}))        

}

/// Function to create a tuple with preimage and hash  

fn get_preimage_and_hash() -> (String,Sha256) 
{
    let mut preimage = [0u8; 32];
    thread_rng().fill(&mut preimage[..]);

    let preimage_str = serialize_hex(&preimage);

    let mut hash = Sha256::engine();
    hash.input(&preimage);
    let payment_hash = Sha256::from_engine(hash);
    
    (preimage_str, payment_hash)

}