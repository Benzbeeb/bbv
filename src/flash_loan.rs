use cosmwasm_std::{
    attr, to_binary, Addr, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, QuerierWrapper,
    Response, StdResult, Uint128, WasmMsg,
};

use crate::error::ContractError;
use crate::msg::{EstimateArbitrageResponse, ExecuteMsg};
use crate::state::{LoanInfo, LOAN_INFO, STATE};
use crate::utils::get_cluster_state;

use astroport::asset::AssetInfo as AstroportAssetInfo;
use astroport::querier::{query_balance, query_pair_info};
use terraswap::asset::{Asset, AssetInfo};

use white_whale::ust_vault::msg::ExecuteMsg as WhiteWhaleExecuteMsg;
use white_whale::ust_vault::msg::FlashLoanPayload;

use std::str::FromStr;

const MULTIPLIER: Uint128 = Uint128::new(10_000u128);
// MULTIPLIER_3 = MULTIPLIER * MULTIPLIER * MULTIPLIER
const MULTIPLIER_3: Uint128 = Uint128::new(1_000_000_000_000u128);

/// ## Description
/// Selects strategy and estimate flash loan amount from cluster info and astroport pool info.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **env** is an object of type [`Env`].
///
/// - **cluster_adddress** is an object type [`String`]. which is the cluster that want to do arbitrage
pub fn try_flash_loan(
    deps: DepsMut,
    info: MessageInfo,
    cluster_address: String,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let validated_cluster_address = deps.api.addr_validate(cluster_address.as_str())?;
    let estimate = query_estimate_arbitrage(deps.as_ref(), cluster_address)?;

    // save loan info to state
    LOAN_INFO.save(
        deps.storage,
        &LoanInfo {
            user_address: info.sender,
            amount: estimate.arbitrage_cost,
            cluster_address: validated_cluster_address.clone(),
            inv: estimate.inv,
            target: estimate.target,
            prices: estimate.prices,
        },
    )?;

    let callback = if estimate.market_price < estimate.intrinsic_price {
        // buy CT from Astroport and redeem
        ExecuteMsg::CallbackRedeem {}
    } else {
        // mint CT and sell on Astroport
        ExecuteMsg::CallbackCreate {}
    };

    let requested_asset = Asset {
        info: AssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        amount: estimate.arbitrage_cost,
    };

    Ok(Response::new()
        .add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: state.vault_address.to_string(),
            msg: to_binary(&WhiteWhaleExecuteMsg::FlashLoan {
                payload: FlashLoanPayload {
                    requested_asset,
                    callback: to_binary(&callback)?,
                },
            })?,
            funds: vec![],
        }))
        .add_attributes(vec![
            attr("key", "value"),
            attr("market", estimate.market_price.to_string()),
            attr("intrinsic", estimate.intrinsic_price.to_string()),
            attr("loan_amount", estimate.arbitrage_cost.to_string()),
        ]))
}

/// ## Description
/// Selects strategy and estimate flash loan amount from cluster info and astroport pool info.
///
/// ## Params
/// - **querier** is a reference to an object of type [`QuerierWrapper`].
///
/// - **loan_amount** is an object of type [`Uint128`].
///
/// - **contract_address** is an object of type [`Addr`].
///
/// - **vault_address** is an object of type [`Addr`].
///
pub fn repay_and_take_profit(
    querier: &QuerierWrapper,
    loan_amount: Uint128,
    contract_address: Addr,
    vault_address: Addr,
) -> StdResult<Vec<CosmosMsg>> {
    let mut messages = vec![];

    // calculates return amount
    // TODO: fix this
    let return_amount = loan_amount.checked_div(Uint128::from(999u128))? + loan_amount;
    messages.push(
        Asset {
            info: AssetInfo::NativeToken {
                denom: "uusd".to_string(),
            },
            amount: return_amount,
        }
        .into_msg(querier, vault_address)?,
    );

    // take profit
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: contract_address.to_string(),
        msg: to_binary(&ExecuteMsg::_UserProfit {})?,
        funds: vec![],
    }));

    Ok(messages)
}

/// ## Description
/// Sends all of profit to user
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **env** is an object of type [`Env`].
///
pub fn try_user_profit(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let loan_info = LOAN_INFO.load(deps.storage)?;
    let amount = query_balance(
        &deps.querier,
        env.contract.address.clone(),
        "uusd".to_string(),
    )?;
    let asset = Asset {
        info: AssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        amount,
    };
    Ok(Response::new()
        .add_message(asset.into_msg(&deps.querier, loan_info.user_address)?)
        .add_attribute("profit", amount.to_string()))
}

/// ## Description
/// Calculates arbitrage information
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **env** is an object of type [`Env`].
///
pub fn query_estimate_arbitrage(
    deps: Deps,
    cluster_address_raw: String,
) -> StdResult<EstimateArbitrageResponse> {
    let state = STATE.load(deps.storage)?;
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
        state.astroport_factory_address,
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
