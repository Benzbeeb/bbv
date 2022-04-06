use cosmwasm_std::{
    coin, to_binary, Addr, CosmosMsg, Deps, QuerierWrapper, QueryRequest, StdResult, WasmMsg,
    WasmQuery,
};

use cw20::Cw20ExecuteMsg;

use crate::msg::{ClusterStateResponse, QueryMsgNebula};

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo};
use astroport::pair::{Cw20HookMsg as AstroportCw20HookMsg, ExecuteMsg as AstroportExecuteMsg};
use astroport::querier::query_pair_info;

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
pub fn swap_to(
    querier: &QuerierWrapper,
    offer_asset: AstroportAsset,
    to_asset: AstroportAssetInfo,
    astroport_factory_address: Addr,
) -> StdResult<CosmosMsg> {
    // query pair contract
    let pair_contract = query_pair_info(
        querier,
        astroport_factory_address,
        &[to_asset, offer_asset.clone().info],
    )?
    .contract_addr
    .to_string();

    match offer_asset.clone().info {
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
            return Ok(message);
        }
    }
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
