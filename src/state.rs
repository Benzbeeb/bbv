use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub ust_vault_address: Addr,
    pub cluster_address: Addr,
    pub incentive_addres: Addr,
    pub astroport_factory_address: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LoanInfo {
    pub user_address: Addr,
    pub amount: Uint128,
}

pub const STATE: Item<State> = Item::new("state");
pub const LOAN_INFO: Item<LoanInfo> = Item::new("state");
