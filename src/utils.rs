use cosmwasm_std::{
    coin, to_binary, Addr, CosmosMsg, Deps, QuerierWrapper, QueryRequest, StdResult, Uint128,
    WasmMsg, WasmQuery,
};

use cw20::Cw20ExecuteMsg;

use crate::msg::{ClusterStateResponse, QueryMsgNebula};

use terra_cosmwasm::{create_swap_msg, TerraMsgWrapper};

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo};
use astroport::pair::{Cw20HookMsg as AstroportCw20HookMsg, ExecuteMsg as AstroportExecuteMsg};
use astroport::querier::query_pair_info;

use moneymarket::market::{Cw20HookMsg as AnchorCw20HookMsg, ExecuteMsg as AnchorExecuteMsg};
/// ## Description
/// Swap token from Astroport pool
///
/// ## Params
/// - **querier** is a reference to an object of type [`QuerierWrapper`].
///
/// - **offer_asset** is an object of type [`AstroportAsset`].
///
/// - **to_asset** is an object of type [`AstroportAssetInfo`].
///
/// - **astroport_factory_address** is an object of type [`Addr`].
///
pub fn create_astroport_swap_msg(
    querier: &QuerierWrapper,
    offer_asset: AstroportAsset,
    to_asset: AstroportAssetInfo,
    astroport_factory_address: Addr,
) -> StdResult<CosmosMsg<TerraMsgWrapper>> {
    // query pair contract
    let pair_contract = query_pair_info(
        querier,
        astroport_factory_address,
        &[to_asset, offer_asset.clone().info],
    )?
    .contract_addr
    .to_string();

    match offer_asset.info.clone() {
        AstroportAssetInfo::Token { contract_addr } => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: contract_addr.to_string(),
                funds: vec![],
                msg: to_binary(&Cw20ExecuteMsg::Send {
                    contract: pair_contract,
                    amount: offer_asset.amount,
                    msg: to_binary(&AstroportCw20HookMsg::Swap {
                        max_spread: None,
                        belief_price: None,
                        to: None,
                    })?,
                })?,
            });
            Ok(message)
        }
        AstroportAssetInfo::NativeToken { denom } => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: pair_contract,
                msg: to_binary(&AstroportExecuteMsg::Swap {
                    offer_asset: offer_asset.clone(),
                    belief_price: None,
                    max_spread: None,
                    to: None,
                })?,
                funds: vec![coin(offer_asset.amount.u128(), denom)],
            });
            Ok(message)
        }
    }
}

pub fn create_terraswap_swap_msg(
    offer_amount: u128,
    offer_denom: String,
    to_denom: String,
) -> StdResult<CosmosMsg<TerraMsgWrapper>> {
    let message = create_swap_msg(coin(offer_amount, offer_denom), to_denom);
    Ok(message)
}

pub fn create_aust_swap_msg(
    anchor_market_contract: Addr,
    aust_contract_contract: Addr,
    amount: Uint128,
    to_ust: bool,
) -> StdResult<CosmosMsg<TerraMsgWrapper>> {
    let message = if to_ust {
        WasmMsg::Execute {
            contract_addr: aust_contract_contract.to_string(),
            msg: to_binary(&Cw20ExecuteMsg::Send {
                contract: anchor_market_contract.to_string(),
                amount,
                msg: to_binary(&AnchorCw20HookMsg::RedeemStable {})?,
            })?,
            funds: vec![],
        }
    } else {
        WasmMsg::Execute {
            contract_addr: anchor_market_contract.to_string(),
            msg: to_binary(&AnchorExecuteMsg::DepositStable {})?,
            funds: vec![coin(amount.u128(), "uusd")],
        }
    };
    Ok(CosmosMsg::Wasm(message))
}

/// ## Description
/// Returns the state of a cluster.
///
/// ## Params
/// - **deps** is an object of type [`Deps`].
///
/// - **cluster** is a reference to an object of type [`Addr`] which is
///     the address of a cluster.
pub fn get_cluster_state(deps: Deps, cluster: &Addr) -> StdResult<ClusterStateResponse> {
    // Query the cluster state
    deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: cluster.to_string(),
        msg: to_binary(&QueryMsgNebula::ClusterState {})?,
    }))
}
