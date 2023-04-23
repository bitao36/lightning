use bitcoin::hashes::HashEngine;
use bitcoin::hashes::Hash;
use bitcoin::consensus::encode::serialize_hex;

//use bitcoin::util::amount::serde::as_btc::deserialize;
use log::{debug};

use std::{
    fmt,
    path::{Path, PathBuf},
};
use rand::Rng;
use rand::thread_rng;
use regex::Regex;
use hex;

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
addholdinvoice <amount_msat> <label> <description> [expiry] || [preimage]

expiry is optional:  It must be minimum 3600 (1 hour) max (24 hours) by default 86400 (24 hours)
preimage is optional: If present, the invoice is created with this preimage

In the optional parameters there can be only one or both. 

example with expiry: 
 addholdinvoice 1000 "001" "With expiry" 7200 
example with preimage 
addholdinvoice 1000 "002" "With preimage" here_valid_preimage
example with both
addholdinvoice 1000 "003" "With both" 20000 here_valid_preimage
*/

pub async fn hodlmethod(plugin: Plugin<PluginState>, v: serde_json::Value) -> Result<serde_json::Value, Error> {
        
    debug!("Parameters from keyboard  {}, rpc file plugin config= {}", v,&plugin.configuration().rpc_file);                
    
    let cltv_delta=plugin.state().cltv_delta.lock().clone();

    if let Some(arr) = v.as_array() 
    {                
        if arr.len()>=3
        {
                
                let amount_msat_wrap=v[0].as_u64().unwrap();
                let amount_msat=Amount::from_msat(amount_msat_wrap);     
                
                let label=  v[1].to_string();
                let description=  v[2].to_string();
                //let description = v.get(2).map(|value| value.to_string()).unwrap_or("Hodl invoice".to_string());
                let mut expiry: u64 = 86400;
                let mut pi: Option<String> = None;              
                                
                if let Some(p) = v.get(3) 
                {
                    if p.is_string() 
                    {

                        let cad3 = p.as_str().unwrap();                        
                        debug!("El parametro3 es una cadena: {}", cad3);    
                        if is_preimage(&cad3) 
                        {
                            debug!("Y es una preimage valida");    
                            //pi=&_cad;
                            pi = Some(String::from(cad3));
                            
                        }
                        else
                        {
                            //lanzar error no es una preimage valida
                            debug!("Pero no es una preimage valida");    
                        }
                    } 
                    else if p.is_u64() 
                    {
                        
                        let num = p.as_u64().unwrap();        
                        debug!("El parametro 3 es un numero: {}", num);     
                        if num>=3600 && num<=86400
                         {
                            expiry=num;
                         }      
                         
                         if let Some(param) = v.get(4) 
                         {
                            if param.is_string() 
                            {

                                let cad4 = param.as_str().unwrap();                        
                                debug!("El parametro4 es una cadena: {}", cad4);    
                                if is_preimage(&cad4) 
                                {
                                    debug!("Y es una preimage valida");                                                                                                                
                                    pi = Some(String::from(cad4));
                                    
                                }
                                else
                                {
                                    //lanzar error no es una preimage valida
                                    debug!("No es una preimage valida");    
                                }
                            }
                        }
                         

                    }
                    else 
                    {
                        debug!("El parámetro no es una cadena ni un número u64");                             
                        
                    }
                }
                
                debug!("expiry={} description={}",expiry,description);                     
                
                if pi.is_some()
                {
                    let hash=get_hash(&pi.clone().unwrap());
                    debug!("Hash={} Preimage={}", hash,pi.clone().unwrap());
                }                 
                //let (pi,hash)= get_preimage_and_hash();                                                                 

                let rpc_path: PathBuf= Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file);                

                let  normal_invoice = invoice(
                    &rpc_path,
                    amount_msat,
                    description,
                    label,
                    Some(expiry),
                    None,
                    pi,
                    None,
                    Some(CLTV_HODL + cltv_delta),
                    None,
                )
                .await?;                                            

                let datastore = datastore(
                    &rpc_path,
                    vec![PLUGIN_NAME.to_string(), normal_invoice.payment_hash.to_string()],
                    Some(Preimagestate::Held.to_string()),
                    None,
                    Some(DatastoreMode::MUST_CREATE),
                    None,
                )
                .await?;

                debug!("In addholdinvoice storing data. DataStore Response = {:?}", datastore);                
                
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
/// and Preimagestate must change to "rejected"

pub async fn cancelmethod(plugin: Plugin<PluginState>,args: serde_json::Value,) -> Result<serde_json::Value, Error> {
    
    let rpc_path: PathBuf= Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file);                
    match args {
        serde_json::Value::Array(a) => {
            if a.len() == 1 
            {   
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
            else 
            {
                return Err(anyhow!("Please provide one payment_hash"));
            }
        }
        _ => return Err(anyhow!("Invalid arguments!")),
    };

    Ok(json!({"result": "success"}))
}

/// example: settleinvoice <hash> || <preimage>
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
                    //log::info!("DataStore handler response = {:?}", resp.datastore);                        
                    debug!("DataStore handler response = {:?}", resp.datastore);                        
                    //Preimagestate::from_str(resp.datastore.first().unwrap().string.as_ref().unwrap()).unwrap()                          
                    resp.datastore                                                          

                },
                Err(e) => 
                {
                    debug!("{} not our invoice: payment_hash: {}", e.to_string(), pay_hash);
                    //log::info!("{} not our invoice: payment_hash: {}", e.to_string(),pay_hash);                                            
                    return Err(anyhow!("Error consultando la DataStore"))
                }
           };           
           //log::info!("Preimage new state = {:?}",);    
           if state_stored.len()>0
           {

               let state_string = state_stored[0].string.as_ref().unwrap();
                //log::info!("Preimage new state = {}",state_string);    
                debug!("Preimage new state = {}",state_string);    

                let preimagestate_value = Preimagestate::from_str(state_string);           
                    
                match preimagestate_value 
                {
                        Some(preimagestate) => 
                        {
                        // Aquí puedes utilizar el valor del enum preimagestate                
                        //log::info!("El valor del enum es: {}", preimagestate);    
                        debug!("El valor del enum es: {}", preimagestate);    

                        // Bloque match para ejecutar diferentes acciones según el valor del enum
                        match preimagestate 
                        {
                            Preimagestate::Held => {
                                debug!("Preimage_State = held" );
                            }
                            Preimagestate::Released => {
                                debug!("Preimage_State = released" );
                            }
                            Preimagestate::Rejected => {
                                debug!("Preimage_State = rejected" );
                            }
                        }
                        }
                    None => 
                    {                
                        //log::info!("La cadena no corresponde a ninguna variante del enum" );
                        debug!("La cadena no corresponde a ninguna variante del enum");
                    }
                }
                return Ok(json!({"PreimageState": state_string}));
           }
           else 
           {            
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


pub async fn getblockheightmethod(plugin: Plugin<PluginState>,_v: serde_json::Value,) -> Result<serde_json::Value, Error> 
{
    let rpc_path: PathBuf= Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file);                
    // usar await? no es recomendado porque en caso de que haya error
    // el programa muestra error y se detiene la ejecución
    //let  bh = getblockheight(&rpc_path).await?;    
    
    let bh = match getblockheight(&rpc_path).await 
    {
        Ok(height) => height,
        Err(error) => {            
            debug!("Error al obtener blockheight: {}", error);                
            0 // Establecer el valor predeterminado en cero
        }
    };

    //En caso de que no importe dar un manejo al error como escribir un log por ejemplo
    //y solamente quiera dejar un valor por defecto en este caso 0 se puede hacer esto
    //let bh = getblockheight(&rpc_path).await.unwrap_or(0);

    Ok(json!({"blockheight": bh}))
}


pub async fn getblockheight(rpc_path: &PathBuf) -> Result<u32, Error> 
{
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let getinfo_request = rpc
        .call(Request::Getinfo(GetinfoRequest {}))
        .await
        .map_err(|e| anyhow!("Error calling get_info: {}", e.to_string()))?;
    match getinfo_request {
        Response::Getinfo(info) => Ok(info.blockheight),
        e => Err(anyhow!("Unexpected result in get_info: {:?}", e)),
    }
}

pub async fn getdeltamethod(plugin: Plugin<PluginState>,_v: serde_json::Value,) -> Result<serde_json::Value, Error> 
{
    let delta=plugin.state().cltv_delta.lock();
    debug!("Delta Value={}",delta);
    Ok(json!({"cltv-delta": *delta}))
}


fn get_hash(preimage_str: &str) -> Sha256 
{
    // Convierte la preimagen de hexadecimal a bytes
    let preimage_bytes = hex::decode(preimage_str).unwrap();

    let mut hash = Sha256::engine();
    hash.input(&preimage_bytes);
    let payment_hash = Sha256::from_engine(hash);
    
    payment_hash
}

fn is_preimage(cadena: &str) -> bool 
{
    // expresión regular para una cadena hexadecimal de 64 caracteres
    let re = Regex::new(r"^[0-9a-fA-F]{64}$").unwrap();
    
    // verificar si la cadena cumple con la expresión regular
    cadena.len() == 64 && re.is_match(cadena)       
}

/// Function to create a tuple with preimage and hash  
#[allow(dead_code)]
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