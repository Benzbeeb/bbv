use cosmwasm_std::{Addr, Decimal, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use astroport::asset::Asset as AstroportAsset;

/// ## Description
/// This structure stores the basic settings for creating a new contract.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InstantiateMsg {
    pub vault_address: String,
    pub incentive_address: String,
    pub astroport_factory_address: String,
    pub aust_token_address: String,
    pub anchor_market_contract: String,
}

/// ## Description
/// This structure describes the execute messages of the contract.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /////////////////////
    /// USER CALLABLE
    /////////////////////

    /// Selects strategy and estimate flash loan amount from cluster info and astroport pool info.
    FlashLoan {
        /// Cluster contract address
        cluster_address: String,
        /// Address to send profit to
        user_address: Option<String>,
    },
    /// Executes arbitrage on Astroport to get CT and perform the redeem operation with flash loan amout.
    _CallbackRedeem {
        /// Cluster contract address
        cluster_address: Addr,
        /// Address to send profit to
        user_address: Addr,
        /// Flash loan amount
        loan_amount: Uint128,
        /// Cluster target
        target: Vec<AstroportAsset>,
    },
    /// Prepares assets for create cluster token.
    _CallbackCreate {
        /// Cluster contract address
        cluster_address: Addr,
        /// Address to send profit to
        user_address: Addr,
        /// Flash loan amount
        loan_amount: Uint128,
        /// Cluster asset target
        target: Vec<AstroportAsset>,
        /// Cluster asset price
        prices: Vec<String>,
    },
    /// Sends all of profit to user
    _UserProfit { user_address: Addr },
    ///  Executes the create operation and uses CT to arbitrage on Astroport with all ralated assets in contract.
    _ArbCreate {
        /// Cluster contract address
        cluster_address: Addr,
        /// Address to send profit to
        user_address: Addr,
        /// Flash loan amount
        loan_amount: Uint128,
        /// Cluster target
        target: Vec<AstroportAsset>,
    },
    /// Swap token to UST from Astroport pool
    _SwapToUstAndTakeProfit {
        /// Address to send profit to
        user_address: Addr,
        /// Flash loan amount
        loan_amount: Uint128,
        /// Cluster target
        target: Vec<AstroportAsset>,
    },

    /////////////////////
    /// OWNER CALLABLE
    /////////////////////
    /// UpdateConfig updates contract setting.
    UpdateConfig {
        /// Whitewhale vault contract address
        vault_address: Option<String>,
        /// Incentive contract address
        incentive_address: Option<String>,
        /// Astroport factory contract address
        astroport_factory_address: Option<String>,
        /// Address to claim the contract ownership
        owner_address: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncentivesMsg {
    /// ArbClusterRedeem executes arbitrage on Astroport to get CT and perform the redeem operation.
    ArbClusterRedeem {
        /// cluster contract
        cluster_contract: String,
        /// UST amount
        asset: AstroportAsset,
        /// minimum returned cluster tokens when arbitraging
        min_cluster: Option<Uint128>,
    },
    /// ArbClusterCreate executes the create operation and uses CT to arbitrage on Astroport.
    ArbClusterCreate {
        /// cluster contract
        cluster_contract: String,
        /// assets offerred for minting
        assets: Vec<AstroportAsset>,
        /// minimum returned UST when arbitraging
        min_ust: Option<Uint128>,
    },
}

// Query Message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    UstVaultAddress {},
    EstimateArbitrage { cluster_address: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct UstVaultAddressResponse {
    /// Whitewhale vault contract address
    pub vault_address: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct EstimateArbitrageResponse {
    /// Price of CT on Astroport
    pub market_price: Decimal,
    /// Intrinsic price
    pub intrinsic_price: Decimal,
    /// Estimate cost to arbitrage
    pub arbitrage_cost: Uint128,
    /// Current inventory / asset balances
    pub inv: Vec<Uint128>,
    /// The current asset target weights
    pub target: Vec<AstroportAsset>,
    /// Prices of the assets in the cluster
    pub prices: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ClusterStateResponse {
    /// The current total supply of the cluster token
    pub outstanding_balance_tokens: Uint128,
    /// Prices of the assets in the cluster
    pub prices: Vec<String>,
    /// Current inventory / asset balances
    pub inv: Vec<Uint128>,
    /// Penalty contract address
    pub penalty: String,
    /// Cluster token address
    pub cluster_token: String,
    /// The current asset target weights
    pub target: Vec<AstroportAsset>,
    /// The address of this cluster contract
    pub cluster_contract_address: String,
    /// The cluster active status - not active if decommissioned
    pub active: bool,
}

/// ## Description
/// This structure describes the available query messages for the cluster contract.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsgNebula {
    /// ClusterState returns the current cluster state.
    ClusterState {},
}
