use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use astroport::asset::Asset as AstroportAsset;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;

//////////////////////////////////////////////////////////////////////
/// STATE
//////////////////////////////////////////////////////////////////////

/// ## Description
/// A custom struct for storing the state contract setting.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub vault_address: Addr,
    pub incentive_addres: Addr,
    pub astroport_factory_address: Addr,
    pub owner_address: Addr,
}

//////////////////////////////////////////////////////////////////////
/// LOAN INFO
//////////////////////////////////////////////////////////////////////

/// ## Description
/// A custom struct for storing the loaner.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LoanInfo {
    pub cluster_address: Addr,
    pub user_address: Addr,
    pub amount: Uint128,
    pub target: Vec<AstroportAsset>,
    pub inv: Vec<Uint128>,
    pub prices: Vec<String>,
}

pub const STATE: Item<State> = Item::new("state");
pub const LOAN_INFO: Item<LoanInfo> = Item::new("loan_info");
