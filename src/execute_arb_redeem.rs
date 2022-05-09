use astroport::querier::{query_balance, query_token_balance};
use cosmwasm_std::{
    coin, to_binary, Addr, CosmosMsg, DepsMut, Env, MessageInfo, Response, Uint128, WasmMsg,
};

use crate::error::ContractError;
use crate::execute_flash_loan::repay_and_take_profit;
use crate::msg::{ExecuteMsg, IncentivesMsg};
use crate::state::STATE;
use crate::utils::{create_astroport_swap_msg, create_aust_swap_msg, create_terraswap_swap_msg};

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo};
use terra_cosmwasm::TerraMsgWrapper;

/// ## Description
/// Executes arbitrage on Astroport to get CT and perform the redeem operation with flash loan amout.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **env** is an object of type [`Env`].
#[allow(clippy::too_many_arguments)]
pub fn try_callback_redeem(
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

    if info.sender != state.vault_address {
        return Err(ContractError::Unauthorized {});
    }

    let asset = astroport::asset::Asset {
        info: AstroportAssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        amount: loan_amount,
    };

    let msgs = vec![
        // Buy cluster from Astroport and redeem with pro-rata
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: state.incentive_addres.to_string(),
            funds: vec![coin(loan_amount.u128(), "uusd".to_string())],
            msg: to_binary(&IncentivesMsg::ArbClusterRedeem {
                cluster_contract: cluster_address.to_string(),
                asset,
                min_cluster: Some(Uint128::from(1u128)),
            })?,
        }),
        // Swap all assets to UST
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            funds: vec![],
            msg: to_binary(&ExecuteMsg::_SwapToUstAndTakeProfit {
                user_address,
                loan_amount,
                target: target.to_vec(),
                profit_threshold,
            })?,
        }),
    ];

    Ok(Response::new().add_messages(msgs))
}

/// ## Description
/// Sell related tokens with cluster to UST, after that repay and take profit.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **env** is an object of type [`Env`].
///
pub fn try_swap_to_ust_and_take_profit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user_address: Addr,
    loan_amount: Uint128,
    target: &[AstroportAsset],
    profit_threshold: Uint128,
) -> Result<Response<TerraMsgWrapper>, ContractError> {
    let state = STATE.load(deps.storage)?;

    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    let mut messages = vec![];
    for asset in target {
        messages.push(match asset.info.clone() {
            AstroportAssetInfo::NativeToken { denom } => {
                if denom == "uusd" {
                    continue;
                }
                create_terraswap_swap_msg(
                    query_balance(&deps.querier, env.contract.address.clone(), denom.clone())?
                        .u128(),
                    denom,
                    "uusd".to_string(),
                )?
            }
            AstroportAssetInfo::Token { contract_addr } => {
                let amount = query_token_balance(
                    &deps.querier,
                    contract_addr.clone(),
                    env.contract.address.clone(),
                )?;
                if contract_addr == state.aust_token_address {
                    create_aust_swap_msg(
                        state.anchor_market_contract.clone(),
                        state.aust_token_address.clone(),
                        amount,
                        true,
                    )?
                } else {
                    create_astroport_swap_msg(
                        &deps.querier,
                        AstroportAsset {
                            info: AstroportAssetInfo::Token {
                                contract_addr: contract_addr.clone(),
                            },
                            amount: query_token_balance(
                                &deps.querier,
                                contract_addr,
                                env.contract.address.clone(),
                            )?,
                        },
                        AstroportAssetInfo::NativeToken {
                            denom: "uusd".to_string(),
                        },
                        state.astroport_factory_address.clone(),
                    )?
                }
            }
        });
    }

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
