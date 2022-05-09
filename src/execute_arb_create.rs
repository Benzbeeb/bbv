use cosmwasm_std::{
    coin, to_binary, Addr, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response, Uint128,
    WasmMsg,
};
use cw20::Cw20ExecuteMsg;
use terra_cosmwasm::TerraMsgWrapper;

use crate::error::ContractError;
use crate::execute_flash_loan::repay_and_take_profit;
use crate::msg::{ExecuteMsg, IncentivesMsg};
use crate::state::STATE;
use crate::utils::{create_astroport_swap_msg, create_aust_swap_msg, create_terraswap_swap_msg};

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo};

use std::str::FromStr;

/// ## Description
/// Prepares assets for create cluster token.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **env** is an object of type [`Env`].
#[allow(clippy::too_many_arguments)]
pub fn try_callback_create(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cluster_address: Addr,
    user_address: Addr,
    loan_amount: Uint128,
    target: &[AstroportAsset],
    prices: &[String],
    profit_threshold: Uint128,
) -> Result<Response<TerraMsgWrapper>, ContractError> {
    let state = STATE.load(deps.storage)?;

    if info.sender != state.vault_address {
        return Err(ContractError::Unauthorized {});
    }

    let value_weights = target
        .iter()
        .zip(prices.iter())
        .map(|(asset, price)| {
            (
                asset.info.clone(),
                asset.amount * Decimal::from_str(price).unwrap(),
            )
        })
        .collect::<Vec<(AstroportAssetInfo, Uint128)>>();
    let total_value_weight: Uint128 = value_weights
        .iter()
        .fold(Uint128::zero(), |total, (_, amount)| total + amount);

    // pro-rata: calculate UST that need to swap to assets based on target weight ratio
    let mut messages = vec![];
    for (asset_info, value_weight) in value_weights {
        if value_weight.is_zero() {
            continue;
        }

        let asset_amount = loan_amount * value_weight / total_value_weight;

        messages.push(match asset_info.clone() {
            AstroportAssetInfo::NativeToken { denom } => {
                // skip if asset if `uusd`
                if denom == "uusd" {
                    continue;
                }
                create_terraswap_swap_msg(asset_amount.u128(), "uusd".to_string(), denom)?
            }
            AstroportAssetInfo::Token { contract_addr } => {
                if contract_addr == state.aust_token_address.clone() {
                    create_aust_swap_msg(
                        state.anchor_market_contract.clone(),
                        state.aust_token_address.clone(),
                        asset_amount,
                        false,
                    )?
                } else {
                    create_astroport_swap_msg(
                        &deps.querier,
                        AstroportAsset {
                            info: AstroportAssetInfo::NativeToken {
                                denom: "uusd".to_string(),
                            },
                            amount: asset_amount,
                        },
                        asset_info.clone(),
                        state.astroport_factory_address.clone(),
                    )?
                }
            }
        });
    }

    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        funds: vec![],
        msg: to_binary(&ExecuteMsg::_ArbCreate {
            cluster_address,
            user_address,
            loan_amount,
            target: target.to_vec(),
            profit_threshold,
        })?,
    }));

    Ok(Response::new().add_messages(messages))
}

/// ## Description
///  Executes the create operation and uses CT to arbitrage on Astroport with all ralated assets in contract.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **env** is an object of type [`Env`].
#[allow(clippy::too_many_arguments)]
pub fn try_arb_create(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cluster_address: Addr,
    user_address: Addr,
    loan_amount: Uint128,
    target: &[AstroportAsset],
    profit_threshold: Uint128,
) -> Result<Response<TerraMsgWrapper>, ContractError> {
    let state = STATE.load(deps.storage)?;

    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    let assets: Vec<AstroportAsset> = target
        .iter()
        .map(|asset| AstroportAsset {
            info: asset.info.clone(),
            // get balance
            amount: asset
                .info
                .query_pool(&deps.querier, env.contract.address.clone())
                .unwrap(),
        })
        .collect();

    let mut funds = vec![];
    let mut messages = vec![];
    for asset in assets.clone() {
        match asset.info {
            AstroportAssetInfo::NativeToken { denom } => {
                funds.push(coin(asset.amount.u128(), denom));
            }
            AstroportAssetInfo::Token { contract_addr } => {
                // increate allowance
                messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: contract_addr.to_string(),
                    msg: to_binary(&Cw20ExecuteMsg::IncreaseAllowance {
                        spender: state.incentive_addres.to_string(),
                        amount: asset.amount,
                        expires: None,
                    })
                    .unwrap(),
                    funds: vec![],
                }));
            }
        }
    }

    funds.sort_by(|c1, c2| c1.denom.cmp(&c2.denom));

    // mint cluster token and sell it on Astroport.
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.incentive_addres.to_string(),
        msg: to_binary(&IncentivesMsg::ArbClusterCreate {
            cluster_contract: cluster_address.to_string(),
            assets,
            min_ust: Some(Uint128::from(1u128)),
        })?,
        funds,
    }));

    // repay and take profit
    messages.append(&mut repay_and_take_profit(
        &deps.querier,
        loan_amount,
        env.contract.address,
        state.vault_address,
        user_address,
        profit_threshold,
    )?);

    Ok(Response::new().add_messages(messages))
}
