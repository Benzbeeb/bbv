#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, QueryRequest, Response, StdResult,
    WasmQuery,
};
use cw2::set_contract_version;

use crate::arb_create::{arb_create, callback_create};
use crate::arb_redeem::{callback_redeem, swap_to_ust_and_take_profit};
use crate::error::ContractError;
use crate::flash_loan::{query_estimate_arbitrage, try_flash_loan};
use crate::msg::{
    ClusterStateResponse, ExecuteMsg, InstantiateMsg, QueryMsg, QueryMsgAstroPort,
    UstVaultAddressResponse,
};
use crate::state::{State, LOAN_INFO, STATE};

use astroport::querier::{query_balance, query_token_balance};
use terraswap::asset::{Asset, AssetInfo};

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

pub fn get_cluster_state(deps: Deps, cluster: &Addr) -> StdResult<ClusterStateResponse> {
    deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: cluster.to_string(),
        msg: to_binary(&QueryMsgAstroPort::ClusterState {})?,
    }))
}
