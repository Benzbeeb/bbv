use cosmwasm_std::{coin, to_binary, CosmosMsg, DepsMut, Env, Response, Uint128, WasmMsg};

use crate::error::ContractError;
use crate::flash_loan::{repay_and_take_profit, swap_to};
use crate::msg::{ExecuteMsg, IncentivesMsg};
use crate::state::{LOAN_INFO, STATE};

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo};

pub fn try_callback_redeem(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let loan_info = LOAN_INFO.load(deps.storage)?;
    let asset = astroport::asset::Asset {
        info: AstroportAssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        amount: loan_info.amount.clone(),
    };

    let msgs = vec![
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: state.incentive_addres.to_string(),
            funds: vec![coin(loan_info.amount.u128(), "uusd".to_string())],
            msg: to_binary(&IncentivesMsg::ArbClusterRedeem {
                cluster_contract: loan_info.cluster_address.to_string(),
                asset,
                min_cluster: Some(Uint128::from(1u128)),
            })?,
        }),
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            funds: vec![],
            msg: to_binary(&ExecuteMsg::SwapToUstAndTakeProfit {})?,
        }),
    ];

    Ok(Response::new().add_messages(msgs))
}

pub fn try_swap_to_ust_and_take_profit(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let loan_info = LOAN_INFO.load(deps.storage)?;
    let mut messages = vec![];

    let asset_infos: Vec<AstroportAssetInfo> =
            loan_info
                    .target
                    .iter()
                    .map(|x| x.info.clone())
                    .filter(|asset_info| {
                        !matches!(asset_info, AstroportAssetInfo::NativeToken { denom } if denom == "uusd")
                    })
                    .collect();

    for asset_info in asset_infos {
        let asset = AstroportAsset {
            info: asset_info.clone(),
            amount: asset_info.query_pool(&deps.querier, env.contract.address.clone())?,
        };

        if asset.amount == Uint128::zero() {
            continue;
        }

        messages.push(swap_to(
            &deps.querier,
            asset,
            AstroportAssetInfo::NativeToken {
                denom: "uusd".to_string(),
            },
            state.astroport_factory_address.clone(),
        )?)
    }

    messages.extend_from_slice(&repay_and_take_profit(
        &deps.querier,
        loan_info.amount,
        env.contract.address,
        state.vault_address,
    )?);

    Ok(Response::new().add_messages(messages))
}
