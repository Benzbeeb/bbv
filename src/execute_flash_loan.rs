use cosmwasm_std::{
    attr, to_binary, Addr, BankMsg, CosmosMsg, DepsMut, Env, MessageInfo, QuerierWrapper, Response,
    StdResult, Uint128, WasmMsg,
};
use terra_cosmwasm::TerraMsgWrapper;

use crate::error::ContractError;
use crate::msg::ExecuteMsg;
use crate::query::estimate_arbitrage;
use crate::state::STATE;

use astroport::querier::query_balance;
use terraswap::asset::{Asset, AssetInfo};

use white_whale::ust_vault::msg::ExecuteMsg as WhiteWhaleExecuteMsg;
use white_whale::ust_vault::msg::FlashLoanPayload;

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
    user_address: Option<String>,
) -> Result<Response<TerraMsgWrapper>, ContractError> {
    let state = STATE.load(deps.storage)?;
    let validated_cluster_address = deps.api.addr_validate(cluster_address.as_str())?;
    let user_address = match user_address {
        Some(addr) => deps.api.addr_validate(addr.as_str())?,
        None => info.sender,
    };

    let estimate = estimate_arbitrage(deps.as_ref(), cluster_address, &state)?;

    let callback = if estimate.market_price < estimate.intrinsic_price {
        // buy CT from Astroport and redeem
        ExecuteMsg::_CallbackRedeem {
            user_address,
            loan_amount: estimate.arbitrage_cost,
            cluster_address: validated_cluster_address.clone(),
            target: estimate.target,
        }
    } else {
        // mint CT and sell on Astroport
        ExecuteMsg::_CallbackCreate {
            user_address,
            loan_amount: estimate.arbitrage_cost,
            cluster_address: validated_cluster_address.clone(),
            target: estimate.target,
            prices: estimate.prices,
        }
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
pub fn repay_and_take_profit(
    querier: &QuerierWrapper,
    loan_amount: Uint128,
    contract_address: Addr,
    vault_address: Addr,
    user_address: Addr,
) -> StdResult<Vec<CosmosMsg<TerraMsgWrapper>>> {
    let mut messages = vec![];

    // calculates return amount
    let return_asset = Asset {
        info: AssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        // TODO: fix this
        amount: loan_amount.checked_div(Uint128::from(999u128))? + loan_amount,
    };

    messages.push(CosmosMsg::Bank(BankMsg::Send {
        to_address: vault_address.to_string(),
        amount: vec![return_asset.deduct_tax(querier)?],
    }));

    // take profit
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: contract_address.to_string(),
        msg: to_binary(&ExecuteMsg::_UserProfit { user_address })?,
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
pub fn try_user_profit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user_address: Addr,
) -> Result<Response<TerraMsgWrapper>, ContractError> {
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

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
        .add_message(CosmosMsg::Bank(BankMsg::Send {
            to_address: user_address.to_string(),
            amount: vec![asset.deduct_tax(&deps.querier)?],
        }))
        .add_attribute("profit", amount.to_string()))
}
