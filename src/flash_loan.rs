use cosmwasm_std::{
    attr, to_binary, CosmosMsg, Decimal, Deps, DepsMut, MessageInfo, Response, StdResult, Uint128,
    WasmMsg,
};

use crate::contract::get_cluster_state;
use crate::error::ContractError;
use crate::msg::{EstimateArbitrageResponse, ExecuteMsg};
use crate::state::{LoanInfo, LOAN_INFO, STATE};

use astroport::asset::AssetInfo as AstroportAssetInfo;
use astroport::querier::query_pair_info;
use terraswap::asset::{Asset, AssetInfo};

use white_whale::ust_vault::msg::ExecuteMsg as WhiteWhaleExecuteMsg;
use white_whale::ust_vault::msg::FlashLoanPayload;

use std::str::FromStr;

const MULTIPLIER: Uint128 = Uint128::new(10_000u128);
const MULTIPLIER_3: Uint128 = Uint128::new(1_000_000_000_000u128);

pub fn try_flash_loan(
    deps: DepsMut,
    info: MessageInfo,
    cluster_address_raw: String,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let cluster_address = deps.api.addr_validate(cluster_address_raw.as_str())?;

    let estimate = query_estimate_arbitrage(deps.as_ref(), cluster_address_raw)?;
    LOAN_INFO.save(
        deps.storage,
        &LoanInfo {
            user_address: info.sender,
            amount: estimate.arbitrage_cost,
            cluster_address: cluster_address.clone(),
            inv: estimate.inv,
            target: estimate.target,
            prices: estimate.prices,
        },
    )?;

    let callback = if estimate.market_price < estimate.intrinsic_price {
        ExecuteMsg::CallbackRedeem {}
    } else {
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

pub fn query_estimate_arbitrage(
    deps: Deps,
    cluster_address_raw: String,
) -> StdResult<EstimateArbitrageResponse> {
    let state = STATE.load(deps.storage)?;
    let cluster_address = deps.api.addr_validate(cluster_address_raw.as_str())?;
    let cluster_state = get_cluster_state(deps, &cluster_address)?;

    let supply: Uint128 = cluster_state.outstanding_balance_tokens;
    let net_asset_val: Uint128 = cluster_state
        .inv
        .iter()
        .zip(cluster_state.prices.iter())
        .map(|(i, p)| (Decimal::from_str(p.as_str()).unwrap()) * i.clone())
        .sum::<Uint128>();

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

    let (ust_amt, ct_amt) = match assets.clone()[0].info {
        AstroportAssetInfo::NativeToken { .. } => (assets[0].amount, assets[1].amount),
        AstroportAssetInfo::Token { .. } => (assets[1].amount, assets[0].amount),
    };

    let intrinsic_price: Decimal = Decimal::from_ratio(net_asset_val, supply);
    let market_price = Decimal::from_ratio(ust_amt, ct_amt);

    let intrinsic_sqrt = intrinsic_price.sqrt() * MULTIPLIER;
    let ct_sqrt = Decimal::from_str(&ct_amt.to_string()).unwrap().sqrt() * MULTIPLIER;
    let ust_sqrt = Decimal::from_str(&ust_amt.to_string()).unwrap().sqrt() * MULTIPLIER;
    let front = intrinsic_sqrt * ct_sqrt * ust_sqrt;

    let arbitrage_cost: Uint128 = if market_price < intrinsic_price {
        front / MULTIPLIER_3 - ust_amt
    } else {
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