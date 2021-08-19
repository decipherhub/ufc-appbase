use std::collections::HashMap;
use std::sync::Arc;

use appbase::*;
use appbase::channel::Sender;
use futures::lock::Mutex as FutureMutex;
use jsonrpc_core::Params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{enumeration, message};
use crate::error::error::ExpectedError;
use crate::libs::opts::opt_to_result;
use crate::libs::request;
use crate::libs::rocks::{get_by_prefix_static, get_static};
use crate::libs::serde::{get_array, get_object, get_str, get_string};
use crate::plugin::jsonrpc::JsonRpcPlugin;
use crate::plugin::mongo::{MongoMsg, MongoPlugin};
use crate::plugin::mysql::{MySqlMsg, MySqlPlugin};
use crate::plugin::rocks::{RocksMethod, RocksMsg, RocksPlugin};
use crate::types::channel::MultiChannel;
use crate::types::enumeration::Enumeration;
use crate::types::mysql::Schema;
use crate::types::subscribe::{SubscribeEvent, SubscribeStatus, SubscribeTarget, SubscribeTask};
use crate::validation::{get_task, resubscribe, stop_subscribe, subscribe, unsubscribe};

pub struct EthereumPlugin {
    sub_events: Option<SubscribeEvents>,
    channels: Option<MultiChannel>,
    monitor: Option<channel::Receiver>,
    schema: Option<HashMap<String, Schema>>,
}

const CHAIN: &str = "ethereum";
const TASK_PREFIX: &str = "task:ethereum";

type SubscribeEvents = Arc<FutureMutex<HashMap<String, SubscribeEvent>>>;

message!((EthereumMsg; {value: Value}); (EthereumMethod; {Subscribe: "subscribe"}, {Resubscribe: "resubscribe"}, {Stop: "stop"}, {Unsubscribe: "unsubscribe"}));

plugin::requires!(EthereumPlugin; JsonRpcPlugin, RocksPlugin);

impl Plugin for EthereumPlugin {
    fn new() -> Self {
        // app::arg(clap::Arg::new("ethereum::block-mysql-sync").long("ether-block-mysql-sync"));
        // app::arg(clap::Arg::new("ethereum::tx-mysql-sync").long("ether-tx-mysql-sync"));
        // app::arg(clap::Arg::new("ethereum::block-mongo-sync").long("ether-block-mongo-sync"));
        // app::arg(clap::Arg::new("ethereum::tx-mongo-sync").long("ether-tx-mongo-sync"));
        // app::arg(clap::Arg::new("ethereum::block-rabbit-mg-sync").long("ether-block-mysql-sync"));
        // app::arg(clap::Arg::new("ethereum::tx-rabbit-mq-sync").long("ether-tx-rabbit-mq-sync"));

        EthereumPlugin {
            sub_events: None,
            channels: None,
            monitor: None,
            schema: None,
        }
    }

    fn initialize(&mut self) {
        self.init();
        self.register_jsonrpc();
        self.load_tasks();
    }

    fn startup(&mut self) {
        let mut monitor = self.monitor.take().unwrap();
        let sub_events = Arc::clone(self.sub_events.as_ref().unwrap());

        let mut rocks_channel = self.channels.as_ref().unwrap().get("rocks");
        // let mysql_channel = self.channels.as_ref().unwrap().get("mysql");
        // let rabbit_channel = self.channels.as_ref().unwrap().get("rabbit");
        // let mongo_channel = self.channels.as_ref().unwrap().get("mongo");
        let app = app::quit_handle().unwrap();

        let schema = self.schema.as_ref().unwrap().clone();

        app::spawn_blocking(move || {
            loop {
                if app.is_quiting() {
                    break;
                }
                let sub_events_try_lock = sub_events.try_lock();
                if sub_events_try_lock.is_none() {
                    continue;
                }
                let mut sub_events_lock = sub_events_try_lock.unwrap();
                if let Ok(msg) = monitor.try_recv() {
                    Self::message_handler(&msg, &mut sub_events_lock, &mut rocks_channel);
                }

                for (_, sub_event) in sub_events_lock.iter_mut() {
                    if sub_event.is_workable() {
                        if sub_event.target == SubscribeTarget::Block {
                            let block_result = Self::poll_block(sub_event);
                            match block_result {
                                Ok(block) => {
                                    println!("event_id={}, block={}", sub_event.event_id(), block.to_string());

                                    Self::sync_event(&rocks_channel, sub_event);
                                    sub_event.curr_height += 1;
                                }
                                Err(err) => Self::error_handler(&rocks_channel, sub_event, err)
                            }
                        } else {
                            let txs_result = Self::poll_txs(sub_event);
                            match txs_result {
                                Ok(txs) => {
                                    for tx in txs {
                                        println!("event_id={}, tx={}", sub_event.event_id(), tx.to_string());
                                    }
                                    Self::sync_event(&rocks_channel, sub_event);
                                    sub_event.curr_height += 1;
                                }
                                Err(err) => Self::error_handler(&rocks_channel, sub_event, err)
                            }
                        }
                    }
                }
            }
        });
    }
    fn shutdown(&mut self) {}
}

impl EthereumPlugin {
    fn init(&mut self) {
        self.sub_events = Some(Arc::new(FutureMutex::new(HashMap::new())));
        let channels = MultiChannel::new(vec!("ethereum", "rocks", "mysql", "rabbit", "mongo"));
        self.channels = Some(channels.to_owned());
        self.monitor = Some(app::subscribe_channel(String::from("ethereum")));
        self.schema = Some(HashMap::new());
    }

    fn register_jsonrpc(&self) {
        let plugin_handle = app::get_plugin::<JsonRpcPlugin>();
        let mut plugin = plugin_handle.lock().unwrap();
        let jsonrpc = plugin.downcast_mut::<JsonRpcPlugin>().unwrap();

        let plugin_handle = app::get_plugin::<RocksPlugin>();
        let mut plugin = plugin_handle.lock().unwrap();
        let rocks = plugin.downcast_mut::<RocksPlugin>().unwrap();

        let eth_channel = self.channels.as_ref().unwrap().get("ethereum");
        let rocks_db = rocks.get_db();

        jsonrpc.add_method(String::from("eth_subscribe"), move |params: Params| {
            let params: Map<String, Value> = params.parse().unwrap();
            let verified = subscribe::verify(&params);
            if verified.is_err() {
                let mut error = Map::new();
                error.insert(String::from("error"), Value::String(verified.unwrap_err().to_string()));
                return Box::new(futures::future::ok(Value::Object(error)));
            }
            let task_id = SubscribeTask::task_id(CHAIN, &params);
            let value = get_static(&rocks_db, task_id.as_str());
            if value.is_null() {
                let message = EthereumMsg::new(EthereumMethod::Subscribe, Value::Object(params.clone()));
                let _ = eth_channel.send(message);

                Box::new(futures::future::ok(Value::String(format!("subscription requested! task_id={}", task_id))))
            } else {
                let mut error = Map::new();
                error.insert(String::from("error"), Value::String(format!("already exist task! task_id={}", task_id)));
                Box::new(futures::future::ok(Value::Object(error)))
            }
        });

        let eth_channel = self.channels.as_ref().unwrap().get("ethereum");
        let rocks_db = rocks.get_db();
        jsonrpc.add_method(String::from("eth_unsubscribe"), move |params: Params| {
            let params: Map<String, Value> = params.parse().unwrap();
            let verified = unsubscribe::verify(&params);
            if verified.is_err() {
                let mut error = Map::new();
                error.insert(String::from("error"), Value::String(verified.unwrap_err().to_string()));
                return Box::new(futures::future::ok(Value::Object(error)));
            }

            let task_id = get_str(&params, "task_id").unwrap();
            let value = get_static(&rocks_db, task_id);
            if value.is_null() {
                let mut error = Map::new();
                error.insert(String::from("error"), Value::String(format!("task does not exist! task_id={}", task_id)));
                Box::new(futures::future::ok(Value::Object(error)))
            } else {
                let eth_msg = EthereumMsg::new(EthereumMethod::Unsubscribe, Value::Object(params.clone()));
                let _ = eth_channel.send(eth_msg);

                Box::new(futures::future::ok(Value::String(format!("unsubscription requested! task_id={}", task_id))))
            }
        });

        let eth_channel = self.channels.as_ref().unwrap().get("ethereum");
        let rocks_db = rocks.get_db();
        jsonrpc.add_method(String::from("eth_resubscribe"), move |params: Params| {
            let params: Map<String, Value> = params.parse().unwrap();
            let verified = resubscribe::verify(&params);
            if verified.is_err() {
                let mut error = Map::new();
                error.insert(String::from("error"), Value::String(verified.unwrap_err().to_string()));
                return Box::new(futures::future::ok(Value::Object(error)));
            }

            let task_id = get_str(&params, "task_id").unwrap();
            let value = get_static(&rocks_db, task_id);
            if value.is_null() {
                let mut error = Map::new();
                error.insert(String::from("error"), Value::String(format!("subscription does not exist! task_id={}", task_id)));
                Box::new(futures::future::ok(Value::Object(error)))
            } else {
                let message = EthereumMsg::new(EthereumMethod::Resubscribe, Value::Object(params.clone()));
                let _ = eth_channel.send(message);

                Box::new(futures::future::ok(Value::String(format!("resubscription requested! task_id={}", task_id))))
            }
        });

        let eth_channel = self.channels.as_ref().unwrap().get("ethereum");
        let rocks_db = rocks.get_db();
        jsonrpc.add_method(String::from("eth_stop_subscription"), move |params: Params| {
            let params: Map<String, Value> = params.parse().unwrap();
            let verified = stop_subscribe::verify(&params);
            if verified.is_err() {
                let mut error = Map::new();
                error.insert(String::from("error"), Value::String(verified.unwrap_err().to_string()));
                return Box::new(futures::future::ok(Value::Object(error)));
            }

            let task_id = get_str(&params, "task_id").unwrap();
            let value = get_static(&rocks_db, task_id);
            if value.is_null() {
                let mut error = Map::new();
                error.insert(String::from("error"), Value::String(format!("task does not exist! task_id={}", task_id)));
                Box::new(futures::future::ok(Value::Object(error)))
            } else {
                let eth_msg = EthereumMsg::new(EthereumMethod::Stop, Value::Object(params.clone()));
                let _ = eth_channel.send(eth_msg);

                Box::new(futures::future::ok(Value::String(format!("stop subscription requested! task_id={}", task_id))))
            }
        });

        let rocks_db = rocks.get_db();
        jsonrpc.add_method(String::from("eth_get_tasks"), move |params: Params| {
            let params: Map<String, Value> = params.parse().unwrap();
            let verified = get_task::verify(&params);
            if verified.is_err() {
                let mut error = Map::new();
                error.insert(String::from("error"), Value::String(verified.unwrap_err().to_string()));
                return Box::new(futures::future::ok(Value::Object(error)));
            }

            let prefix = match params.get("task_id") {
                None => TASK_PREFIX,
                Some(task_id) => task_id.as_str().unwrap(),
            };
            let tasks = get_by_prefix_static(&rocks_db, prefix);
            Box::new(futures::future::ok(tasks))
        });
    }

    fn load_tasks(&self) {
        let plugin_handle = app::get_plugin::<RocksPlugin>();
        let mut plugin = plugin_handle.lock().unwrap();
        let rocks = plugin.downcast_mut::<RocksPlugin>().unwrap();

        let rocks_db = rocks.get_db();
        let raw_tasks = get_by_prefix_static(&rocks_db, TASK_PREFIX);
        let sub_events = Arc::clone(self.sub_events.as_ref().unwrap());
        raw_tasks.as_array().unwrap().iter()
            .for_each(|raw_task| {
                let task = raw_task.as_object().unwrap();
                let event = SubscribeEvent::from(task);
                let mut sub_events_lock = sub_events.try_lock().unwrap();
                sub_events_lock.insert(event.task_id.clone(), event);
            });
    }

    fn sync_event(rocks_channel: &channel::Sender, sub_event: &mut SubscribeEvent) {
        let task = SubscribeTask::from(&sub_event, String::from(""));
        let task_id = task.task_id.clone();

        let msg = RocksMsg::new(RocksMethod::Put, task_id, Value::String(json!(task).to_string()));
        let _ = rocks_channel.send(msg);
    }

    fn error_handler(rocks_channel: &channel::Sender, sub_event: &mut SubscribeEvent, error: ExpectedError) {
        match error {
            ExpectedError::BlockHeightError(err_msg) => println!("{}", err_msg),
            ExpectedError::FilterError(err_msg) => {
                println!("{}", err_msg);
                Self::sync_event(&rocks_channel, sub_event);
                sub_event.curr_height += 1;
            }
            _ => {
                sub_event.handle_error(&rocks_channel, error.to_string());
            }
        };
    }

    fn message_handler(msg: &Value, sub_events: &mut HashMap<String, SubscribeEvent>, rocks_channel: &mut Sender) {
        let parsed_msg = msg.as_object().unwrap();
        let method = EthereumMethod::find(get_str(parsed_msg, "method").unwrap()).unwrap();
        let params = get_object(parsed_msg, "value").unwrap();
        match method {
            EthereumMethod::Subscribe => {
                let new_event = SubscribeEvent::new(CHAIN, &params);
                sub_events.insert(new_event.task_id.clone(), new_event.clone());

                let task = SubscribeTask::from(&new_event, String::from(""));
                let msg = RocksMsg::new(RocksMethod::Put, new_event.task_id, Value::String(json!(task).to_string()));
                let _ = rocks_channel.send(msg);
            }
            EthereumMethod::Unsubscribe => {
                let task_id = get_string(&params, "task_id").unwrap();
                sub_events.remove(&task_id);

                let msg = RocksMsg::new(RocksMethod::Delete, task_id, Value::Null);
                let _ = rocks_channel.send(msg);
            }
            EthereumMethod::Resubscribe => {
                let task_id = get_str(&params, "task_id").unwrap();
                let mut sub_event = sub_events.get(task_id).unwrap().clone();
                sub_event.node_idx = 0;
                sub_event.status = SubscribeStatus::Working;
                sub_events.insert(sub_event.task_id.clone(), sub_event.clone());

                let task = SubscribeTask::from(&sub_event, String::from(""));
                let msg = RocksMsg::new(RocksMethod::Put, sub_event.task_id, Value::String(json!(task).to_string()));
                let _ = rocks_channel.send(msg);
            }
            EthereumMethod::Stop => {
                let task_id = get_str(&params, "task_id").unwrap();
                let mut sub_event = sub_events.get(task_id).unwrap().clone();
                sub_event.status = SubscribeStatus::Stopped;
                sub_events.insert(sub_event.task_id.clone(), sub_event.clone());

                let task = SubscribeTask::from(&sub_event, String::from(""));
                let msg = RocksMsg::new(RocksMethod::Put, sub_event.task_id, Value::String(json!(task).to_string()));
                let _ = rocks_channel.send(msg);
            }
        };
    }

    fn poll_block(sub_event: &mut SubscribeEvent) -> Result<Value, ExpectedError> {
        let node_index = usize::from(sub_event.node_idx);
        let req_url = sub_event.nodes[node_index].clone();
        let hex_height = format!("0x{:X}", sub_event.curr_height);
        let req_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [ hex_height, true ],
            "id": 1
        });
        let body = request::post(req_url.as_str(), req_body.to_string().as_str())?;
        let block = opt_to_result(body.get("result"))?;
        if block.is_null() {
            return Err(ExpectedError::BlockHeightError(String::from("block has not yet been created!")));
        } else {
            Ok(block.clone())
        }
    }

    fn poll_txs(sub_event: &mut SubscribeEvent) -> Result<Vec<Value>, ExpectedError> {
        let block_value = Self::poll_block(sub_event)?;
        let block = opt_to_result(block_value.as_object())?;
        let transactions = get_array(block, "transactions")?;
        Ok(transactions.clone())
    }
}

