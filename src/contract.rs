#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
use cw2::set_contract_version;
use terra_cosmwasm::TerraMsgWrapper;

use crate::error::ContractError;
use crate::execute_arb_create::{try_arb_create, try_callback_create};
use crate::execute_arb_redeem::{try_callback_redeem, try_swap_to_ust_and_take_profit};
use crate::execute_flash_loan::{try_flash_loan, try_user_profit};
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, UstVaultAddressResponse};
use crate::query::query_estimate_arbitrage;
use crate::state::{State, STATE};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:bbv";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// ## Description
/// Creates a new contract with the specified parameters packed in the `msg` variable.
/// Returns a [`Response`] with the specified attributes if the operation was successful,
/// or a [`ContractError`] if the contract was not created.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **_env** is an object of type [`Env`].
///
/// - **_info** is an object of type [`MessageInfo`].
///
/// - **msg**  is a message of type [`InstantiateMsg`] which contains the parameters used for creating the contract.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        vault_address: deps.api.addr_validate(msg.vault_address.as_str())?,
        incentive_addres: deps.api.addr_validate(msg.incentive_address.as_str())?,
        astroport_factory_address: deps
            .api
            .addr_validate(msg.astroport_factory_address.as_str())?,
        aust_token_address: deps.api.addr_validate(msg.aust_token_address.as_str())?,
        anchor_market_contract: deps
            .api
            .addr_validate(msg.anchor_market_contract.as_str())?,
        owner_address: info.sender,
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    STATE.save(deps.storage, &state)?;
    Ok(Response::new())
}

/// ## Description
/// Exposes all the execute functions available in the contract.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **env** is an object of type [`Env`].
///
/// - **info** is an object of type [`MessageInfo`].
///
/// - **msg** is an object of type [`ExecuteMsg`].
///
/// ## Commands
/// - **ExecuteMsg::FlashLoan { cluster_address
///         }** Select a strategy and estimate cost amount to arbitrage.
///
/// - **ExecuteMsg::CallbackRedeem {}** Redeem actions to be performed with the loaned funds.
///
/// - **ExecuteMsg::CallbackCreate{}** Create actions to be performed with the loaned funds.
///
/// - **ExecuteMsg::ArbCreate {}** Increases allowances and sends funds to call ArbClusterCreate.
///
/// - **ExecuteMsg::_UserProfit {}** Sends all profit to user.
///
/// - **ExecuteMsg::WithdrawNative {
///             send_to,
///             denom,
///         }** Sends all native to send_to.
///
/// - **ExecuteMsg::WithdrawToken {
///             send_to,
///             denom,
///         }** Sends all token to send_to.
///
/// - **ExecuteMsg::SwapToUstAndTakeProfit {}** Swaps all asset to UST after that take a profit.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<TerraMsgWrapper>, ContractError> {
    match msg {
        ExecuteMsg::FlashLoan { cluster_address } => try_flash_loan(deps, info, cluster_address),
        ExecuteMsg::_CallbackRedeem {
            cluster_address,
            user_address,
            loan_amount,
            target,
        } => try_callback_redeem(
            deps,
            env,
            info,
            cluster_address,
            user_address,
            loan_amount,
            &target,
        ),
        ExecuteMsg::_CallbackCreate {
            cluster_address,
            user_address,
            loan_amount,
            target,
            prices,
        } => try_callback_create(
            deps,
            env,
            info,
            cluster_address,
            user_address,
            loan_amount,
            &target,
            &prices,
        ),
        ExecuteMsg::_ArbCreate {
            cluster_address,
            user_address,
            loan_amount,
            target,
        } => try_arb_create(
            deps,
            env,
            info,
            cluster_address,
            user_address,
            loan_amount,
            &target,
        ),
        ExecuteMsg::_UserProfit { user_address } => try_user_profit(deps, env, info, user_address),
        ExecuteMsg::UpdateConfig {
            vault_address,
            incentive_address,
            astroport_factory_address,
            owner_address,
        } => try_update_config(
            deps,
            info,
            vault_address,
            incentive_address,
            astroport_factory_address,
            owner_address,
        ),
        ExecuteMsg::_SwapToUstAndTakeProfit {
            user_address,
            loan_amount,
            target,
        } => try_swap_to_ust_and_take_profit(deps, env, info, user_address, loan_amount, &target),
    }
}

/// ## Description
/// Updates general contract configurations. Returns a [`ContractError`] on failure.
///
/// ## Params
/// - **deps** is an object of type [`DepsMut`].
///
/// - **info** is an object of type [`MessageInfo`].
///
/// - **vault_address** is an object of type [`Option<String>`] which is the address of
///     the new White whale vault contract.
///
/// - **incentive_addres** is an object of type [`Option<String>`] which is the address of
///     the new incentive contract.
///
/// - **astroport_factory_address** is an object of type [`Option<String>`] which is the address of
///     the new astroport factory contract.
///
/// - **owner_address** is an object of type [`Option<String>`] which is a new owner address to update.
///
/// ## Executor
/// Only the owner can execute this.
pub fn try_update_config(
    deps: DepsMut,
    info: MessageInfo,
    vault_address: Option<String>,
    incentive_addres: Option<String>,
    astroport_factory_address: Option<String>,
    owner_address: Option<String>,
) -> Result<Response<TerraMsgWrapper>, ContractError> {
    let mut state = STATE.load(deps.storage)?;

    if info.sender != state.owner_address {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(vault_address) = vault_address {
        state.vault_address = deps.api.addr_validate(vault_address.as_ref())?;
    }
    if let Some(incentive_addres) = incentive_addres {
        state.incentive_addres = deps.api.addr_validate(incentive_addres.as_ref())?;
    }
    if let Some(astroport_factory_address) = astroport_factory_address {
        state.astroport_factory_address =
            deps.api.addr_validate(astroport_factory_address.as_ref())?;
    }
    if let Some(owner_address) = owner_address {
        state.owner_address = deps.api.addr_validate(owner_address.as_str())?;
    }

    STATE.save(deps.storage, &state)?;
    Ok(Response::new())
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
