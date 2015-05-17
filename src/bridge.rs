use hyper::Client;
use hyper::client::Body;
use hyper::client::response::Response;
use disco;
use serde::json;
use serde::json::value::Value;
use serde::{ Serialize, Deserialize };
use errors::HueError;
use errors::AppError;
use regex::Regex;
use std::str::FromStr;
use std::io::Read;
use std::collections::BTreeMap;

#[derive(Debug,Clone,Deserialize)]
pub struct LightState {
    pub on: bool,
    pub bri: u8,
    pub hue: u16,
    pub sat: u8,
    pub effect: String,
    pub xy: (f32,f32),
    #[serde(default)] pub ct: u16,
    pub alert: String,
    pub colormode: String,
    pub reachable: bool,
}

#[derive(Debug,Clone,Deserialize)]
pub struct Light {
    pub name: String,
    pub modelid: String,
    pub swversion: String,
    pub uniqueid: String,
    pub state: LightState,
    #[serde(rename="type")] pub _type:String,
    pub manufacturername:String,
    pub pointsymbol: BTreeMap<String,Value>
}

#[derive(Debug,Clone)]
pub struct IdentifiedLight {
    pub id: usize,
    pub light: Light,
}

#[derive(Debug,Clone,Copy,Serialize,Deserialize)]
pub struct CommandLight {
    pub on:Option<bool>,
    pub bri:Option<u8>,
    pub hue:Option<u16>,
    pub sat:Option<u8>,
    pub transitiontime:Option<u16>,
}

impl CommandLight {
    pub fn empty() -> CommandLight {
        CommandLight { on:None, bri:None, hue:None, sat:None, transitiontime:None }
    }
    pub fn on() -> CommandLight {
        CommandLight { on:Some(true), ..CommandLight::empty() }
    }
    pub fn off() -> CommandLight {
        CommandLight { on:Some(false), ..CommandLight::empty() }
    }
    pub fn with_bri(&self, b:u8) -> CommandLight {
        CommandLight { bri:Some(b), ..*self }
    }
    pub fn with_hue(&self, h:u16) -> CommandLight {
        CommandLight { hue:Some(h), ..*self }
    }
    pub fn with_sat(&self, s:u8) -> CommandLight {
        CommandLight { sat:Some(s), ..*self }
    }
}

#[derive(Debug)]
pub struct Bridge {
    ip: String,
    username: Option<String>,
}

impl Bridge {
    #[allow(dead_code)]
    pub fn discover() -> Option<Bridge> {
        disco::discover_hue_bridge().ok().map( |i| Bridge{ ip:i, username:None } )
    }

    pub fn discover_required() -> Bridge {
        Bridge::discover().unwrap_or_else( || panic!("No bridge found!") )
    }

    pub fn with_user(self, username:String) -> Bridge {
        Bridge{ username: Some(username), ..self }
    }

    pub fn register_user(&self, devicetype:&str, username:&str) -> Result<Value,HueError> {
        if username.len() < 10 || username.len() > 40 {
            return Err(HueError::StdError("username must be between 10 and 40 characters".to_string()))
        }
        #[derive(Deserialize, Serialize)]
        struct PostApi {
            devicetype: String,
            username:String
        }
        let obtain = PostApi {
            devicetype:devicetype.to_string(),
            username:username.to_string()
        };
        let body = try!(json::to_string(&obtain));
        let mut client = Client::new();
        let url = format!("http://{}/api", self.ip);
        let mut resp = try!(client.post(&url[..])
            .body(Body::BufBody(body.as_bytes(), body.as_bytes().len())).send());
        self.parse_write_resp(&mut resp)
    }

    pub fn get_all_lights(&self) -> Result<Vec<IdentifiedLight>,HueError> {
        let url = format!("http://{}/api/{}/lights",
            self.ip, self.username.clone().unwrap());
        let mut client = Client::new();
        let mut resp = try!(client.get(&url[..]).send());
        let mut body = String::new();
        try!(resp.read_to_string(&mut body));
        let json:BTreeMap<String,Light> = try!(json::from_str(&*body));
        let lights:Result<Vec<IdentifiedLight>,HueError> = json.iter().map( |entry| {
            let id:usize = try!(entry.0.parse());
            Ok(IdentifiedLight{ id:id, light:entry.1.clone() })
        }).collect();
        let mut lights = try!(lights);
        lights.sort_by( |a,b| a.id.cmp(&b.id) );
        Ok(lights)
    }

    pub fn set_light_state(&self, light:usize, command:CommandLight) -> Result<Value, HueError> {
        let url = format!("http://{}/api/{}/lights/{}/state",
            self.ip, self.username.clone().unwrap(), light);
        let body = try!(json::to_string(&command));
        let re1 = Regex::new("\"[a-z]*\":null").unwrap();
        let cleaned1 = re1.replace_all(&body,"");
        let re2 = Regex::new(",+").unwrap();
        let cleaned2 = re2.replace_all(&cleaned1,",");
        let re3 = Regex::new(",\\}").unwrap();
        let cleaned3 = re3.replace_all(&cleaned2,"}");
        let re3 = Regex::new("\\{,").unwrap();
        let cleaned4 = re3.replace_all(&cleaned3,"{");
        let mut client = Client::new();
        let mut resp = try!(client.put(&url[..])
            .body(Body::BufBody(cleaned4.as_bytes(), cleaned4.as_bytes().len())).send());
        self.parse_write_resp(&mut resp)
    }

    fn parse_write_resp(&self, resp:&mut Response) -> Result<Value,HueError> {
        let mut body = String::new();
        try!(resp.read_to_string(&mut body));
        let json:Value = try!(json::from_str(&*body));

        let objects = try!(json.as_array()
            .ok_or(HueError::StdError("expected array".to_string())));
        if objects.len() == 0 {
            return Err(HueError::StdError("expected non-empty array".to_string()));
        }
        let object = try!(objects[0].as_object()
            .ok_or(HueError::StdError("expected first item to be an object".to_string())));
        let obj = object.get(&"error".to_string()).and_then( |o| o.as_object() );
        match obj {
            Some(e) => {
                Err(HueError::BridgeError(AppError{
                    address: e.get("address").and_then(|s| s.as_string()).unwrap_or("").to_string(),
                    description: e.get("description").and_then(|s| s.as_string()).unwrap_or("").to_string(),
                    code: e.get("type").and_then(|s| s.as_u64()).unwrap_or(0) as u8
                }))
            },
            None => Ok(json.clone())
        }
    }
}


