use bitcoin::hashes::HashEngine;
use bitcoin::hashes::Hash;
use bitcoin::consensus::encode::serialize_hex;

use std::{
    fmt,
    path::{Path, PathBuf},
};

use rand::Rng;
use rand::thread_rng;

use anyhow::{anyhow};

use cln_rpc::{
    model::*,
    primitives::{Amount,AmountOrAny,Sha256},
    ClnRpc,
};

use cln_plugin::{Error, Plugin};

use crate::config::{PluginState};

pub const PLUGIN_NAME:&str="hodlinvoice";
pub const CLTV_HODL:u32=163;

///The states of the invoice will be determined by the state in which the preimage is 

#[derive(Debug, Clone)]
pub enum Preimagestate {
    Held,
    Released,
    Rejected,
}
impl Preimagestate {
    pub fn to_string(&self) -> String {
        match self {
            Preimagestate::Held => "held".to_string(),
            Preimagestate::Released => "released".to_string(),
            Preimagestate::Rejected => "rejected".to_string(),
        }
    }
}
impl Preimagestate {
    pub fn from_str(s: &str) -> Option<Preimagestate> {
        match s.to_lowercase().as_str() {
            "held" => Some(Preimagestate::Held),
            "released" => Some(Preimagestate::Released),
            "rejected" => Some(Preimagestate::Rejected),
            _ => None,
        }
    }
}

impl fmt::Display for Preimagestate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Preimagestate::Held => write!(f, "Held"),
            Preimagestate::Released => write!(f, "Released"),
            Preimagestate::Rejected => write!(f, "Rejected"),
        }
    }
}

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

pub async fn hodlmethod(plugin: Plugin<PluginState>, v: serde_json::Value) -> Result<serde_json::Value, Error> {
    
    log::info!("Parametros obtenidos de holdinvoice  {}, plugin config= {}", v,&plugin.configuration().rpc_file);
    
    let cltv_delta=plugin.state().cltv_delta.lock().clone();

    if let Some(arr) = v.as_array() 
    {                
        if arr.len()>=2
        {
                
                let amount_msat_wrap=v[0].as_u64().unwrap();
                let amount_msat=Amount::from_msat(amount_msat_wrap);     
                
                let label=  v[1].to_string();
                let expiry: u64=3600;                
                let description=v[2].to_string();

                let (pi,hash)= get_preimage_and_hash();                                  

                log::info!("Hash={} Preimage={}", hash,pi);

                let rpc_path: PathBuf= Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file);                

                let  normal_invoice = invoice(
                    &rpc_path,
                    amount_msat,
                    description,
                    label,
                    Some(expiry),
                    None,
                    Some(pi),
                    None,
                    Some(CLTV_HODL + cltv_delta),
                    None,
                )
                .await?;                                            

                let _datastore = datastore(
                    &rpc_path,
                    vec![PLUGIN_NAME.to_string(), normal_invoice.payment_hash.to_string()],
                    Some(Preimagestate::Held.to_string()),
                    None,
                    Some(DatastoreMode::MUST_CREATE),
                    None,
                )
                .await?;

                log::info!("In addholdinvoice store DataStore response = {:?}", _datastore);                                                                    
                
                 Ok(json!(normal_invoice))                
        }
        else 
        {            
            Ok(json!("Missing parameters!"))            
        }         
    }
    else 
    {

        Ok(json!("You must enter parameters!"))
    }   
    
          
}


/// Example cancelinvoice <hash>
/// The invoice must be searched with the hash
/// and Preimagestate must change to "canceled"

pub async fn cancelmethod(plugin: Plugin<PluginState>,args: serde_json::Value,) -> Result<serde_json::Value, Error> {
    
    let rpc_path: PathBuf= Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file);                
    match args {
        serde_json::Value::Array(a) => {
            if a.len() != 1 {
                return Err(anyhow!("Please provide one payment_hash"));
            } else {
                match a.first().unwrap() {
                    serde_json::Value::String(i) => {
                        datastore(
                            &rpc_path,
                            vec![PLUGIN_NAME.to_string(), i.clone()],
                            Some(Preimagestate::Rejected.to_string()),
                            None,
                            Some(DatastoreMode::MUST_REPLACE),
                            None,
                        )
                        .await?;
                    }
                    _ => return Err(anyhow!("Invalid string!")),
                };
            }
        }
        _ => return Err(anyhow!("Invalid arguments!")),
    };

    Ok(json!({"result": "success"}))
}

/// example: settleinvoice <hash or preimage>
/// The invoice must be searched with the hash or preimage
/// and Preimagestate must change to "released"


pub async fn settlemethod(plugin: Plugin<PluginState>,args: serde_json::Value,) -> Result<serde_json::Value, Error> 
{
    let rpc_path: PathBuf= Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file);                
    match args {
        serde_json::Value::Array(arg) => {
            if arg.len() == 1 
            {
                match arg.first().unwrap() 
                {
                    serde_json::Value::String(pay_hash) => {
                        datastore(
                            &rpc_path,
                            vec![PLUGIN_NAME.to_string(), pay_hash.clone()],
                            Some(Preimagestate::Released.to_string()),
                            None,
                            Some(DatastoreMode::MUST_REPLACE),
                            None,
                        )
                        .await?;
                    }
                    _ => return Err(anyhow!("Invalid string!")),
                };
            } 
            else 
            {
                return Err(anyhow!("Please provide one payment_hash!"));
                
            }
        }
        _ => return Err(anyhow!("Invalid arguments!")),
    };

    Ok(json!({"result": "success"}))
}

pub async fn listinvoices(rpc_path: &PathBuf,label: Option<String>,payment_hash: Option<String>) -> Result<ListinvoicesResponse, Error> 
{
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let invoice_request = rpc
        .call(Request::ListInvoices(ListinvoicesRequest {
            label,
            invstring: None,
            payment_hash,
            offer_id: None,
        }))
        .await
        .map_err(|e| anyhow!("Error calling listinvoices: {:?}", e))?;
    match invoice_request {
        Response::ListInvoices(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in listinvoices: {:?}", e)),
    }
}

pub async fn datastore(rpc_path: &PathBuf,key: Vec<String>,string: Option<String>, hex: Option<String>,    mode: Option<DatastoreMode>,    generation: Option<u64>,) -> Result<DatastoreResponse, Error> 
{
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let datastore_request = rpc
        .call(Request::Datastore(DatastoreRequest {
            key,
            string,
            hex,
            mode,
            generation,
        }))
        .await
        .map_err(|e| anyhow!("Error calling datastore: {:?}", e))?;
    match datastore_request {
        Response::Datastore(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in datastore: {:?}", e)),
    }
}


pub async fn listdatastore(rpc_path: &PathBuf,key: Option<Vec<String>>,) -> Result<ListdatastoreResponse, Error> 
{
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let datastore_request = rpc
        .call(Request::ListDatastore(ListdatastoreRequest { key }))
        .await
        .map_err(|e| anyhow!("Error calling listdatastore: {:?}", e))?;
    match datastore_request {
        Response::ListDatastore(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in listdatastore: {:?}", e)),
    }
}


pub async fn invoice(rpc_path: &PathBuf,amount_msat: Amount,description: String, label: String,
    expiry: Option<u64>,fallbacks: Option<Vec<String>>, preimage: Option<String>,
    exposeprivatechannels: Option<bool>, cltv: Option<u32>, 
    deschashonly: Option<bool>,) -> Result<InvoiceResponse, Error> 
{
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let invoice_request = rpc
        .call(Request::Invoice(InvoiceRequest {
            amount_msat: AmountOrAny::Amount(amount_msat),
            description,
            label,
            expiry,
            fallbacks,
            preimage,
            exposeprivatechannels,
            cltv,
            deschashonly,
        }))
        .await
        .map_err(|e| anyhow!("Error calling invoice: {:?}", e))?;
    match invoice_request 
    {
        Response::Invoice(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in invoice: {:?}", e)),
    }
}


/// La idea es permitir obtener el estado de la preimagen almacenada
 
pub async fn getkeyfromstore(plugin: Plugin<PluginState>,v: serde_json::Value,) -> Result<serde_json::Value, Error> 
{
    
    if let Some(arr) = v.as_array() 
    {        
        
        if arr.len()==1
        {
            let pay_hash: &str=v[0].as_str().unwrap_or("");    
            log::info!("payment_hash obtenido = {}", pay_hash);
            let rpc_path: PathBuf= Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file);    

            let state_stored= match listdatastore(&rpc_path, Some(vec![PLUGIN_NAME.to_string(), pay_hash.to_string()])).await 
            {
                Ok(resp) => 
                {   
                    log::info!("DataStore handler response = {:?}", resp.datastore);                        
                    //Preimagestate::from_str(resp.datastore.first().unwrap().string.as_ref().unwrap()).unwrap()                          
                    resp.datastore                                                          

                },
                Err(e) => 
                {
                    //debug!("{} not our invoice: payment_hash: {}", e.to_string(), pay_hash);
                    log::info!("{} not our invoice: payment_hash: {}", e.to_string(),pay_hash);                                            
                    return Err(anyhow!("Error consultando la DataStore"))
                }
           };           
           //log::info!("Preimage new state = {:?}",);    
           if state_stored.len()>0
           {

               let state_string = state_stored[0].string.as_ref().unwrap();
                log::info!("Preimage new state = {}",state_string);    

                let preimagestate_value = Preimagestate::from_str(state_string);           
                    
                match preimagestate_value 
                {
                        Some(preimagestate) => 
                        {
                        // Aquí puedes utilizar el valor del enum preimagestate                
                        log::info!("El valor del enum es: {}", preimagestate);    
                        // Bloque match para ejecutar diferentes acciones según el valor del enum
                        match preimagestate 
                        {
                            Preimagestate::Held => {
                                log::info!("Preimage_State = held" );
                            }
                            Preimagestate::Released => {
                                log::info!("Preimage_State = released" );
                            }
                            Preimagestate::Rejected => {
                                log::info!("Preimage_State = rejected" );
                            }
                        }
                        }
                    None => 
                    {                
                        log::info!("La cadena no corresponde a ninguna variante del enum" );
                    }
                }
                return Ok(json!({"PreimageState": state_string}));
           }
           else 
           {
            log::info!("Hash not found!");
            Ok(json!("Hash not found!"))            
           }
        }
        else 
         {
            Ok(json!("Please provide one payment_hash!"))            
        }
    }
    else 
    {
        Ok(json!("Missing parameters!"))            
    }
            
  

}


pub async fn getdeltamethod(plugin: Plugin<PluginState>,_v: serde_json::Value,) -> Result<serde_json::Value, Error> 
{
    let delta=plugin.state().cltv_delta.lock();
    log::info!("Delta Value={}",delta);
    Ok(json!({"cltv-delta": *delta}))
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