#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    coin, to_binary, Addr, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo, QueryRequest, Reply,
    ReplyOn, Response, StdResult, SubMsg, Uint128, WasmMsg, WasmQuery,
};
use cw2::set_contract_version;
use cw20::Cw20ExecuteMsg;

use crate::error::ContractError;
use crate::msg::{
    ClusterStateResponse, ExecuteMsg, IncentivesMsg, InstantiateMsg, QueryMsg, QueryMsgAstroPort,
    UstVaultAddressResponse,
};
use crate::state::{State, STATE};

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo, PairInfo};
use astroport::pair::{Cw20HookMsg as AstroportCw20HookMsg, ExecuteMsg as AstroportExecuteMsg};
use astroport::querier::{query_balance, query_pair_info, query_token_balance};
use terraswap::asset::{Asset, AssetInfo};
use white_whale::ust_vault::msg::ExecuteMsg as WhiteWhaleExecuteMsg;
use white_whale::ust_vault::msg::FlashLoanPayload;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:bbv";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        ust_vault_address: deps.api.addr_validate(msg.ust_vault_address.as_str())?,
        cluster_address: deps.api.addr_validate(msg.cluster_address.as_str())?,
        incentive_addres: deps.api.addr_validate(msg.incentive_address.as_str())?,
        user_address: None,
        astroport_factory_address: deps
            .api
            .addr_validate(msg.astroport_factory_address.as_str())?,
        loan_amount: None,
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    STATE.save(deps.storage, &state)?;

    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::FlashLoan { amount } => try_flash_loan(deps, amount, info),
        ExecuteMsg::_CallbackRedeem {} => callback_redeem(deps),
        ExecuteMsg::_UserProfit {} => _user_profit(deps, env),
    }
}

pub fn try_flash_loan(
    deps: DepsMut,
    amount: Uint128,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    STATE.update(deps.storage, |mut state| -> Result<State, ContractError> {
        state.user_address = Some(info.sender);
        state.loan_amount = Some(amount);
        Ok(state)
    })?;

    let requested_asset = Asset {
        info: AssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        amount: Uint128::from(amount),
    };

    Ok(
        Response::new().add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: state.ust_vault_address.to_string(),
            msg: to_binary(&WhiteWhaleExecuteMsg::FlashLoan {
                payload: FlashLoanPayload {
                    requested_asset,
                    callback: to_binary(&ExecuteMsg::_CallbackRedeem {})?,
                },
            })?,
            funds: vec![],
        })),
    )
}

pub fn callback_redeem(deps: DepsMut) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let loan_amount = state.loan_amount.unwrap();
    let asset = astroport::asset::Asset {
        info: AstroportAssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        amount: loan_amount,
    };

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.incentive_addres.to_string(),
        funds: vec![coin(loan_amount.u128(), "uusd".to_string())],
        msg: to_binary(&IncentivesMsg::ArbClusterRedeem {
            cluster_contract: state.cluster_address.to_string(),
            asset,
            min_cluster: Some(Uint128::from(1u128)),
        })?,
    });

    Ok(Response::new().add_submessage(SubMsg {
        msg,
        gas_limit: None,
        id: 1,
        reply_on: ReplyOn::Success,
    }))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let mut messages = vec![];

    match msg.id {
        1 => {
            let cluster_asset_infos: Vec<AstroportAssetInfo> =
                get_cluster_state(deps.as_ref(), &state.cluster_address)?
                    .target
                    .iter()
                    .map(|x| x.info.clone())
                    .collect();

            for cluster_asset_info in cluster_asset_infos {
                let asset = AstroportAsset {
                    info: cluster_asset_info.clone(),
                    amount: match cluster_asset_info {
                        AstroportAssetInfo::Token { contract_addr } => query_token_balance(
                            &deps.querier,
                            contract_addr.clone(),
                            env.contract.address.clone(),
                        )?,
                        AstroportAssetInfo::NativeToken { denom } => query_balance(
                            &deps.querier,
                            env.contract.address.clone(),
                            denom.clone(),
                        )?,
                    },
                };

                if asset.amount == Uint128::zero() {
                    continue;
                }

                match asset.clone().info {
                    AstroportAssetInfo::Token { contract_addr } => {
                        let asset_infos = [
                            AstroportAssetInfo::NativeToken {
                                denom: "uusd".to_string(),
                            },
                            AstroportAssetInfo::Token { contract_addr },
                        ];

                        // Load Astroport pair info
                        let pair_info: PairInfo = query_pair_info(
                            &deps.querier,
                            state.astroport_factory_address.clone(),
                            &asset_infos,
                        )?;

                        let message = CosmosMsg::Wasm(WasmMsg::Execute {
                            contract_addr: asset.info.to_string(),
                            msg: to_binary(&Cw20ExecuteMsg::Send {
                                contract: pair_info.contract_addr.to_string(),
                                amount: asset.amount,
                                msg: to_binary(&AstroportCw20HookMsg::Swap {
                                    max_spread: None,
                                    belief_price: None,
                                    to: None,
                                })?,
                            })?,
                            funds: vec![],
                        });
                        messages.push(message)
                    }
                    AstroportAssetInfo::NativeToken { denom } => {
                        if denom != "uusd" {
                            let asset_infos = [
                                AstroportAssetInfo::NativeToken {
                                    denom: "uusd".to_string(),
                                },
                                AstroportAssetInfo::NativeToken {
                                    denom: denom.clone(),
                                },
                            ];

                            // Load Astroport pair info
                            let pair_info: PairInfo = query_pair_info(
                                &deps.querier,
                                state.astroport_factory_address.clone(),
                                &asset_infos,
                            )?;

                            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: pair_info.contract_addr.to_string(),
                                msg: to_binary(&AstroportExecuteMsg::Swap {
                                    offer_asset: asset.clone(),
                                    belief_price: None,
                                    max_spread: None,
                                    to: None,
                                })?,
                                funds: vec![coin(asset.amount.u128(), denom)],
                            });
                            messages.push(message)
                        }
                    }
                }
            }
            let loan_amount = state.loan_amount.unwrap();
            let return_amount =
                loan_amount.checked_div(Uint128::from(99u128)).unwrap() + loan_amount;
            messages.push(
                Asset {
                    info: AssetInfo::NativeToken {
                        denom: "uusd".to_string(),
                    },
                    amount: return_amount,
                }
                .into_msg(&deps.querier, state.ust_vault_address)?,
            );
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_binary(&ExecuteMsg::_UserProfit {})?,
                funds: vec![],
            }))
        }
        _ => return Err(ContractError::Unauthorized {}),
    }
    Ok(Response::new().add_messages(messages))
}

pub fn _user_profit(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let uusd_ja = query_balance(
        &deps.querier,
        env.contract.address.clone(),
        "uusd".to_string(),
    )?;
    let asset = Asset {
        info: AssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        amount: uusd_ja,
    };
    Ok(Response::new()
        .add_message(asset.into_msg(&deps.querier, state.user_address.unwrap())?)
        .add_attribute("profit", uusd_ja.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::UstVaultAddress {} => to_binary(&query_ust_vault_address(deps)?),
    }
}

fn query_ust_vault_address(deps: Deps) -> StdResult<UstVaultAddressResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(UstVaultAddressResponse {
        ust_vault_address: state.ust_vault_address,
    })
}

pub fn get_cluster_state(deps: Deps, cluster: &Addr) -> StdResult<ClusterStateResponse> {
    deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: cluster.to_string(),
        msg: to_binary(&QueryMsgAstroPort::ClusterState {})?,
    }))
}
