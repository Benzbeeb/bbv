use cosmwasm_std::{Decimal, Deps, StdResult, Uint128};

use crate::msg::EstimateArbitrageResponse;
use crate::state::{State, STATE};
use crate::utils::get_cluster_state;

use astroport::asset::AssetInfo as AstroportAssetInfo;
use astroport::querier::query_pair_info;

use std::str::FromStr;

const MULTIPLIER: Uint128 = Uint128::new(10_000u128);
// MULTIPLIER_3 = MULTIPLIER * MULTIPLIER * MULTIPLIER
const MULTIPLIER_3: Uint128 = Uint128::new(1_000_000_000_000u128);

/// ## Description
/// Query estimate arbitrage amount.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **cluster_address_raw** is an object of type [`String`].
pub fn query_estimate_arbitrage(
    deps: Deps,
    cluster_address_raw: String,
) -> StdResult<EstimateArbitrageResponse> {
    let state = STATE.load(deps.storage)?;
    estimate_arbitrage(deps, cluster_address_raw, &state)
}

/// ## Description
/// Calculates arbitrage information
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **cluster_address_raw** is an object of type [`String`].
///
/// - **state** is a reference to an object of type [`State`].
pub fn estimate_arbitrage(
    deps: Deps,
    cluster_address_raw: String,
    state: &State,
) -> StdResult<EstimateArbitrageResponse> {
    let cluster_address = deps.api.addr_validate(cluster_address_raw.as_str())?;
    let cluster_state = get_cluster_state(deps, &cluster_address)?;

    let supply: Uint128 = cluster_state.outstanding_balance_tokens;
    // net_asset_val = Prices dot Inventory
    let net_asset_val: Uint128 = cluster_state
        .inv
        .iter()
        .zip(cluster_state.prices.iter())
        .map(|(i, p)| (Decimal::from_str(p.as_str()).unwrap()) * i.clone())
        .sum::<Uint128>();

    // query pool info
    let pool_info = query_pair_info(
        &deps.querier,
        state.astroport_factory_address.clone(),
        &[
            AstroportAssetInfo::NativeToken {
                denom: "uusd".to_string(),
            },
            AstroportAssetInfo::Token {
                contract_addr: deps
                    .api
                    .addr_validate(cluster_state.cluster_token.as_str())?,
            },
        ],
    )?;

    let assets = pool_info.query_pools(&deps.querier, pool_info.contract_addr.clone())?;

    // get UST amount and CT amount
    let (ust_amt, ct_amt) = match assets.clone()[0].info {
        AstroportAssetInfo::NativeToken { .. } => (assets[0].amount, assets[1].amount),
        AstroportAssetInfo::Token { .. } => (assets[1].amount, assets[0].amount),
    };
    // intrinsic_price = net_asset_val / supply
    let intrinsic_price: Decimal = Decimal::from_ratio(net_asset_val, supply);
    // market_price = ust_amt / ct_amt
    let market_price = Decimal::from_ratio(ust_amt, ct_amt);

    let intrinsic_sqrt = intrinsic_price.sqrt() * MULTIPLIER;
    let ct_sqrt = Decimal::from_str(&ct_amt.to_string()).unwrap().sqrt() * MULTIPLIER;
    let ust_sqrt = Decimal::from_str(&ust_amt.to_string()).unwrap().sqrt() * MULTIPLIER;
    let front = intrinsic_sqrt * ct_sqrt * ust_sqrt;

    let arbitrage_cost: Uint128 = if market_price < intrinsic_price {
        // sqrt(intrinsic_price * ct_amt * ust_amt) - ust_amt
        front / MULTIPLIER_3 - ust_amt
    } else {
        // sqrt(intrinsic_price * ct_amt * ust_amt) - ct_amt * intrinsic_price
        let back = ct_amt * MULTIPLIER_3 * intrinsic_price;
        (front - back) / MULTIPLIER_3
    };
    Ok(EstimateArbitrageResponse {
        market_price,
        intrinsic_price,
        arbitrage_cost,
        inv: cluster_state.inv,
        target: cluster_state.target,
        prices: cluster_state.prices,
    })
}
