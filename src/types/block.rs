use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::types::block::SubscribeStatus::{Requested, Working};

#[derive(Debug, Clone)]
pub struct SubscribeBlock {
    pub chain: String,
    pub chain_id: String,
    pub start_height: u64,
    pub current_height: u64,
    pub nodes: Vec<String>,
    pub node_index: u16,
    pub status: SubscribeStatus,
}

impl SubscribeBlock {
    pub fn new(chain: String, params: &Map<String, Value>) -> SubscribeBlock {
        let nodes = params.get("nodes").unwrap().as_array().unwrap().iter().map(|n| { String::from(n.as_str().unwrap()) }).collect();
        let start_height = params.get("start_height").unwrap().as_u64().unwrap();
        SubscribeBlock {
            chain,
            chain_id: String::from(params.get("chain_id").unwrap().as_str().unwrap()),
            start_height,
            current_height: start_height,
            nodes,
            node_index: 0,
            status: SubscribeStatus::Requested,
        }
    }

    pub fn request_url(&self) -> String {
        let node_index = usize::from(self.node_index);
        self.nodes[node_index].to_string() + self.current_height.to_string().as_str()
    }

    pub fn is_workable(&self) -> bool {
        vec!(Requested, Working).contains(&self.status)
    }

    pub fn task_id(&self) -> String {
        format!("{}:{}", self.chain, self.chain_id)
    }

    pub fn block_id(&self) -> String {
        format!("{}:{}:{}", self.chain, self.chain_id, self.current_height)
    }

    pub fn has_fallback(&self) -> bool {
        usize::from(self.node_index) + 1 < self.nodes.len()
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct BlockTask {
    pub task_id: String,
    pub chain: String,
    pub chain_id: String,
    pub start_height: u64,
    pub nodes: Vec<String>,
}

impl BlockTask {
    pub fn new(chain: String, params: &Map<String, Value>) -> BlockTask {
        let nodes = params.get("nodes").unwrap().as_array().unwrap().iter().map(|n| { String::from(n.as_str().unwrap()) }).collect();
        BlockTask {
            task_id: format!("{}:{}", chain, params.get("chain_id").unwrap().as_str().unwrap()),
            chain,
            chain_id: String::from(params.get("chain_id").unwrap().as_str().unwrap()),
            start_height: params.get("start_height").unwrap().as_u64().unwrap(),
            nodes,
        }
    }

    pub fn nodes_str(&self) -> String {
        format!("{:?}", self.nodes)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum SubscribeStatus {
    Requested,
    Working,
    Error,
}
