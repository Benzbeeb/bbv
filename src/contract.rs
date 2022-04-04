#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coin, to_binary, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo,
    QuerierWrapper, QueryRequest, Response, StdResult, Uint128, WasmMsg, WasmQuery,
};
use cw2::set_contract_version;
use cw20::Cw20ExecuteMsg;

use crate::error::ContractError;
use crate::msg::{
    ClusterStateResponse, EstimateArbitrageResponse, ExecuteMsg, IncentivesMsg, InstantiateMsg,
    QueryMsg, QueryMsgAstroPort, UstVaultAddressResponse,
};
use crate::state::{LoanInfo, State, LOAN_INFO, STATE};

use astroport::asset::{Asset as AstroportAsset, AssetInfo as AstroportAssetInfo};
use astroport::pair::{Cw20HookMsg as AstroportCw20HookMsg, ExecuteMsg as AstroportExecuteMsg};
use astroport::querier::{query_balance, query_pair_info, query_token_balance};
use terraswap::asset::{Asset, AssetInfo};
use white_whale::ust_vault::msg::ExecuteMsg as WhiteWhaleExecuteMsg;
use white_whale::ust_vault::msg::FlashLoanPayload;

use std::str::FromStr;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:bbv";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MULTIPLIER: Uint128 = Uint128::new(10_000u128);
const MULTIPLIER_3: Uint128 = Uint128::new(1_000_000_000_000u128);

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        vault_address: deps.api.addr_validate(msg.vault_address.as_str())?,
        incentive_addres: deps.api.addr_validate(msg.incentive_address.as_str())?,
        astroport_factory_address: deps
            .api
            .addr_validate(msg.astroport_factory_address.as_str())?,
        owner_address: deps.api.addr_validate(msg.owner_address.as_str())?,
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
        ExecuteMsg::FlashLoan { cluster_address } => try_flash_loan(deps, info, cluster_address),
        ExecuteMsg::CallbackRedeem {} => callback_redeem(deps, env),
        ExecuteMsg::_UserProfit {} => _user_profit(deps, env),
        ExecuteMsg::CallbackCreate {} => callback_create(deps, env),
        ExecuteMsg::ArbCreate {} => arb_create(deps, env),
        ExecuteMsg::UpdateConfig {
            vault_address,
            incentive_address,
            astroport_factory_address,
            owner_address,
        } => update_config(
            deps,
            info,
            vault_address,
            incentive_address,
            astroport_factory_address,
            owner_address,
        ),
        ExecuteMsg::WithdrawNative { send_to, denom } => {
            withdraw_native(deps, env, info, send_to, denom)
        }
        ExecuteMsg::WithdrawToken {
            send_to,
            contract_address,
        } => withdraw_token(deps, env, info, send_to, contract_address),
        ExecuteMsg::SwapToUstAndTakeProfit {} => swap_to_ust_and_take_profit(deps, env),
    }
}

pub fn withdraw_native(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    send_to: String,
    denom: String,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    if state.owner_address != info.sender {
        return Err(ContractError::Unauthorized {});
    }
    let amount = query_balance(&deps.querier, env.contract.address, denom.clone())?;

    Ok(Response::new().add_message(
        Asset {
            info: AssetInfo::NativeToken { denom },
            amount,
        }
        .into_msg(&deps.querier, deps.api.addr_validate(send_to.as_ref())?)?,
    ))
}

pub fn withdraw_token(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    send_to: String,
    contract_address: String,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    if state.owner_address != info.sender {
        return Err(ContractError::Unauthorized {});
    }
    let amount = query_token_balance(
        &deps.querier,
        env.contract.address,
        deps.api.addr_validate(contract_address.as_ref())?,
    )?;

    Ok(Response::new().add_message(
        Asset {
            info: AssetInfo::Token {
                contract_addr: contract_address,
            },
            amount,
        }
        .into_msg(&deps.querier, deps.api.addr_validate(send_to.as_ref())?)?,
    ))
}

pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    vault_address_raw: Option<String>,
    incentive_addres_raw: Option<String>,
    astroport_factory_address_raw: Option<String>,
    owner_address_raw: Option<String>,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    if state.owner_address.clone() != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    let new_state = State {
        vault_address: if let Some(new_vault_address) = vault_address_raw {
            deps.api.addr_validate(new_vault_address.as_ref())?
        } else {
            state.vault_address
        },
        incentive_addres: if let Some(new_incentive_addres) = incentive_addres_raw {
            deps.api.addr_validate(new_incentive_addres.as_ref())?
        } else {
            state.incentive_addres
        },
        astroport_factory_address: if let Some(new_astroport_factory_address) =
            astroport_factory_address_raw
        {
            deps.api
                .addr_validate(new_astroport_factory_address.as_ref())?
        } else {
            state.astroport_factory_address
        },
        owner_address: if let Some(new_owner_address) = owner_address_raw {
            deps.api.addr_validate(new_owner_address.as_ref())?
        } else {
            state.owner_address
        },
    };
    STATE.save(deps.storage, &new_state)?;
    Ok(Response::new())
}

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

pub fn callback_redeem(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
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

pub fn swap_to_ust_and_take_profit(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
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
        QueryMsg::EstimateArbitrage { cluster_address } => {
            to_binary(&query_estimate_arbitrage(deps, cluster_address)?)
        }
    }
}

fn query_vault_address(deps: Deps) -> StdResult<UstVaultAddressResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(UstVaultAddressResponse {
        vault_address: state.vault_address,
    })
}

fn query_estimate_arbitrage(
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

pub fn get_cluster_state(deps: Deps, cluster: &Addr) -> StdResult<ClusterStateResponse> {
    deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: cluster.to_string(),
        msg: to_binary(&QueryMsgAstroPort::ClusterState {})?,
    }))
}
