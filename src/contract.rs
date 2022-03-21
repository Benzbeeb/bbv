#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coin, to_binary, Addr, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo,
    QuerierWrapper, QueryRequest, Reply, ReplyOn, Response, StdResult, SubMsg, Uint128, WasmMsg,
    WasmQuery,
};
use cw2::set_contract_version;
use cw20::Cw20ExecuteMsg;

use crate::error::ContractError;
use crate::msg::{
    ClusterStateResponse, ExecuteMsg, IncentivesMsg, InstantiateMsg, QueryMsg, QueryMsgAstroPort,
    UstVaultAddressResponse,
};
use crate::state::{LoanInfo, State, LOAN_INFO, STATE};

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo};
use astroport::pair::{Cw20HookMsg as AstroportCw20HookMsg, ExecuteMsg as AstroportExecuteMsg};
use astroport::querier::{query_balance, query_pair_info};
use terraswap::asset::{Asset, AssetInfo};
use white_whale::ust_vault::msg::ExecuteMsg as WhiteWhaleExecuteMsg;
use white_whale::ust_vault::msg::FlashLoanPayload;

use std::str::FromStr;

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
        vault_address: deps.api.addr_validate(msg.vault_address.as_str())?,
        cluster_address: deps.api.addr_validate(msg.cluster_address.as_str())?,
        incentive_addres: deps.api.addr_validate(msg.incentive_address.as_str())?,
        astroport_factory_address: deps
            .api
            .addr_validate(msg.astroport_factory_address.as_str())?,
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
        ExecuteMsg::CallbackRedeem {} => callback_redeem(deps),
        ExecuteMsg::_UserProfit {} => _user_profit(deps, env),
        ExecuteMsg::CallbackCreate {} => callback_create(deps, env),
        ExecuteMsg::ArbCreate {} => arb_create(deps, env),
    }
}

pub fn try_flash_loan(
    deps: DepsMut,
    amount: Uint128,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    LOAN_INFO.save(
        deps.storage,
        &LoanInfo {
            user_address: info.sender,
            amount,
        },
    )?;

    let requested_asset = Asset {
        info: AssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        amount: Uint128::from(amount),
    };

    let cluster_state = get_cluster_state(deps.as_ref(), &state.cluster_address)?;

    let supply: Uint128 = cluster_state.outstanding_balance_tokens;
    let net_asset_val: Uint128 = cluster_state
        .inv
        .iter()
        .zip(cluster_state.prices.iter())
        .map(|(i, p)| (Decimal::from_str(p.as_str()).unwrap()) * i.clone())
        .sum::<Uint128>();

    let intrinsic: Decimal = Decimal::from_ratio(net_asset_val, supply);

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
    let market = match assets.clone()[0].info {
        AstroportAssetInfo::NativeToken { .. } => {
            Decimal::from_ratio(assets[0].amount, assets[1].amount)
        }
        AstroportAssetInfo::Token { .. } => Decimal::from_ratio(assets[1].amount, assets[0].amount),
    };

    let callback = if market < intrinsic {
        ExecuteMsg::CallbackRedeem {}
    } else {
        ExecuteMsg::CallbackCreate {}
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
            attr("market", market.to_string()),
            attr("intrinsic", intrinsic.to_string()),
        ]))
}

pub fn callback_redeem(deps: DepsMut) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let loan_info = LOAN_INFO.load(deps.storage)?;
    let asset = astroport::asset::Asset {
        info: AstroportAssetInfo::NativeToken {
            denom: "uusd".to_string(),
        },
        amount: loan_info.amount.clone(),
    };

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.incentive_addres.to_string(),
        funds: vec![coin(loan_info.amount.u128(), "uusd".to_string())],
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

pub fn callback_create(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let loan_info = LOAN_INFO.load(deps.storage)?;

    let cluster_state = get_cluster_state(deps.as_ref(), &state.cluster_address)?;
    let total_asset_amount: Uint128 = cluster_state.clone().inv.iter().sum();

    let asset_amounts = cluster_state
        .inv
        .iter()
        .map(|&inv| Decimal::from_ratio(inv, total_asset_amount) * (loan_info.amount.clone()));

    let target_infos = cluster_state.target.iter().map(|x| x.info.clone());
    let mut messages: Vec<CosmosMsg> = vec![];

    for (info, amount) in target_infos.zip(asset_amounts) {
        if matches!(info.clone(),AstroportAssetInfo::NativeToken { denom } if denom == "uusd" )
            || amount.is_zero()
        {
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
            info,
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

    let assets: Vec<AstroportAsset> = get_cluster_state(deps.as_ref(), &state.cluster_address)?
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

    let allowances: Vec<CosmosMsg> = assets
        .clone()
        .iter()
        .filter_map(|asset| match asset.info.clone() {
            AstroportAssetInfo::NativeToken { .. } => None,
            AstroportAssetInfo::Token { contract_addr } => {
                return Some(CosmosMsg::Wasm(WasmMsg::Execute {
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
        })
        .collect();

    messages.extend_from_slice(&allowances);

    let funds: Vec<Coin> = assets
        .clone()
        .iter()
        .filter_map(|asset| match asset.info.clone() {
            AstroportAssetInfo::NativeToken { denom } => Some(coin(asset.amount.u128(), denom)),
            AstroportAssetInfo::Token { .. } => None,
        })
        .collect();

    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.incentive_addres.to_string(),
        msg: to_binary(&IncentivesMsg::ArbClusterCreate {
            cluster_contract: state.cluster_address.to_string(),
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

pub fn swap_to(
    querier: &QuerierWrapper,
    offer_asset: AstroportAsset,
    to_asset: AstroportAssetInfo,
    astroport_factory_address: Addr,
) -> StdResult<CosmosMsg> {
    let pair_contract = query_pair_info(
        querier,
        astroport_factory_address,
        &[to_asset, offer_asset.clone().info],
    )?
    .contract_addr
    .to_string();

    match offer_asset.clone().info {
        AstroportAssetInfo::Token { contract_addr } => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: contract_addr.to_string(),
                funds: vec![],
                msg: to_binary(&Cw20ExecuteMsg::Send {
                    contract: pair_contract,
                    amount: offer_asset.amount,
                    msg: to_binary(&AstroportCw20HookMsg::Swap {
                        max_spread: None,
                        belief_price: None,
                        to: None,
                    })?,
                })?,
            });
            Ok(message)
        }
        AstroportAssetInfo::NativeToken { denom } => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: pair_contract,
                msg: to_binary(&AstroportExecuteMsg::Swap {
                    offer_asset: offer_asset.clone(),
                    belief_price: None,
                    max_spread: None,
                    to: None,
                })?,
                funds: vec![coin(offer_asset.amount.u128(), denom)],
            });
            return Ok(message);
        }
    }
}

pub fn repay_and_take_profit(
    querier: &QuerierWrapper,
    loan_amount: Uint128,
    contract_address: Addr,
    vault_address: Addr,
) -> StdResult<Vec<CosmosMsg>> {
    let mut messages = vec![];

    let return_amount = loan_amount.checked_div(Uint128::from(999u128)).unwrap() + loan_amount;
    messages.push(
        Asset {
            info: AssetInfo::NativeToken {
                denom: "uusd".to_string(),
            },
            amount: return_amount,
        }
        .into_msg(querier, vault_address)?,
    );

    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: contract_address.to_string(),
        msg: to_binary(&ExecuteMsg::_UserProfit {})?,
        funds: vec![],
    }));

    Ok(messages)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let loan_info = LOAN_INFO.load(deps.storage)?;
    let mut messages = vec![];

    match msg.id {
        1 => {
            let asset_infos: Vec<AstroportAssetInfo> =
                get_cluster_state(deps.as_ref(), &state.cluster_address)?
                    .target
                    .iter()
                    .map(|x| x.info.clone())
                    .filter(|asset_info| {
                        matches!(asset_info, AstroportAssetInfo::NativeToken { denom } if denom == "uusd")
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
        }
        _ => return Err(ContractError::Unauthorized {}),
    }
    Ok(Response::new().add_messages(messages))
}

pub fn _user_profit(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
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

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::UstVaultAddress {} => to_binary(&query_vault_address(deps)?),
    }
}

fn query_vault_address(deps: Deps) -> StdResult<UstVaultAddressResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(UstVaultAddressResponse {
        vault_address: state.vault_address,
    })
}

pub fn get_cluster_state(deps: Deps, cluster: &Addr) -> StdResult<ClusterStateResponse> {
    deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: cluster.to_string(),
        msg: to_binary(&QueryMsgAstroPort::ClusterState {})?,
    }))
}
