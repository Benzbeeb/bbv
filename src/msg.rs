use cosmwasm_std::{Addr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use astroport::asset::Asset;

// Instantiate Message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InstantiateMsg {
    pub vault_address: String,
    pub incentive_address: String,
    pub astroport_factory_address: String,
    pub owner_address: String,
}

// Execute Message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    FlashLoan {
        cluster_address: String,
    },
    CallbackRedeem {},
    _UserProfit {},
    CallbackCreate {},
    ArbCreate {},
    UpdateConfig {
        vault_address: Option<String>,
        incentive_address: Option<String>,
        astroport_factory_address: Option<String>,
        owner_address: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncentivesMsg {
    ArbClusterRedeem {
        cluster_contract: String,
        asset: Asset,
        min_cluster: Option<Uint128>,
    },
    ArbClusterCreate {
        cluster_contract: String,
        assets: Vec<Asset>,
        min_ust: Option<Uint128>,
    },
}

// Query Message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    UstVaultAddress {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct UstVaultAddressResponse {
    pub vault_address: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ClusterStateResponse {
    pub outstanding_balance_tokens: Uint128,
    pub prices: Vec<String>,
    pub inv: Vec<Uint128>,
    pub penalty: String,
    pub cluster_token: String,
    pub target: Vec<Asset>,
    pub cluster_contract_address: String,
    pub active: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsgAstroPort {
    ClusterState {},
}
