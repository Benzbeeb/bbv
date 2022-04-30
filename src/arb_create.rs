use cosmwasm_std::{coin, to_binary, CosmosMsg, DepsMut, Env, Response, Uint128, WasmMsg};
use cw20::Cw20ExecuteMsg;

use crate::error::ContractError;
use crate::flash_loan::repay_and_take_profit;
use crate::msg::{ExecuteMsg, IncentivesMsg};
use crate::state::{LOAN_INFO, STATE};
use crate::utils::swap_to;

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo};

/// ## Description
/// Prepares assets for create cluster token.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **env** is an object of type [`Env`].
///
pub fn try_callback_create(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let loan_info = LOAN_INFO.load(deps.storage)?;

    let total_weight_amount: Uint128 = loan_info
        .target
        .clone()
        .iter()
        .map(|asset| asset.amount)
        .sum();

    // pro-rata: calculate UST that need to swap to assets based on target weight ratio
    let asset_amounts = loan_info
        .target
        .iter()
        .map(|asset| asset.amount * loan_info.amount.clone() / total_weight_amount);

    let mut messages: Vec<CosmosMsg> = vec![];
    for (asset, amount) in loan_info.target.iter().zip(asset_amounts) {
        if let AstroportAssetInfo::NativeToken { denom } = asset.info.clone() {
            // skip if asset if `uusd`
            if denom == "uusd" {
                continue;
            }
        }

        if amount.is_zero() {
            continue;
        }
        // swap UST to asset
        messages.push(swap_to(
            &deps.querier,
            AstroportAsset {
                info: AstroportAssetInfo::NativeToken {
                    denom: "uusd".to_string(),
                },
                amount,
            },
            asset.info.clone(),
            state.astroport_factory_address.clone(),
        )?);
    }

    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        funds: vec![],
        msg: to_binary(&ExecuteMsg::ArbCreate {})?,
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
///
pub fn try_arb_create(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let mut messages: Vec<CosmosMsg> = vec![];
    let state = STATE.load(deps.storage)?;
    let loan_info = LOAN_INFO.load(deps.storage)?;

    let assets: Vec<AstroportAsset> = loan_info
        .target
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
            cluster_contract: loan_info.cluster_address.to_string(),
            assets,
            min_ust: Some(Uint128::from(1u128)),
        })?,
        funds,
    }));

    // repay and take profit
    messages.extend_from_slice(&repay_and_take_profit(
        &deps.querier,
        loan_info.amount,
        env.contract.address.clone(),
        state.vault_address,
    )?);

    Ok(Response::new().add_messages(messages))
}
