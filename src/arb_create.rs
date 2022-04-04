use cosmwasm_std::{coin, to_binary, CosmosMsg, DepsMut, Env, Response, Uint128, WasmMsg};
use cw20::Cw20ExecuteMsg;

use crate::error::ContractError;
use crate::flash_loan::{repay_and_take_profit, swap_to};
use crate::msg::{ExecuteMsg, IncentivesMsg};
use crate::state::{LOAN_INFO, STATE};

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo};

pub fn callback_create(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let loan_info = LOAN_INFO.load(deps.storage)?;

    let total_asset_amount: Uint128 = loan_info.inv.clone().iter().sum();
    let asset_amounts = loan_info
        .inv
        .iter()
        .map(|&inv| inv * loan_info.amount.clone() / total_asset_amount);

    let mut messages: Vec<CosmosMsg> = vec![];
    for (asset, amount) in loan_info.target.iter().zip(asset_amounts) {
        if let AstroportAssetInfo::NativeToken { denom } = asset.info.clone() {
            if denom == "uusd" {
                continue;
            }
        }

        if amount.is_zero() {
            continue;
        }

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

pub fn arb_create(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let mut messages: Vec<CosmosMsg> = vec![];
    let state = STATE.load(deps.storage)?;
    let loan_info = LOAN_INFO.load(deps.storage)?;

    let assets: Vec<AstroportAsset> = loan_info
        .target
        .iter()
        .map(|asset| AstroportAsset {
            info: asset.info.clone(),
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

    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.incentive_addres.to_string(),
        msg: to_binary(&IncentivesMsg::ArbClusterCreate {
            cluster_contract: loan_info.cluster_address.to_string(),
            assets,
            min_ust: Some(Uint128::from(1u128)),
        })?,
        funds,
    }));

    messages.extend_from_slice(&repay_and_take_profit(
        &deps.querier,
        loan_info.amount,
        env.contract.address.clone(),
        state.vault_address,
    )?);

    Ok(Response::new().add_messages(messages))
}
