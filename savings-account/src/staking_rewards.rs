elrond_wasm::imports!();
elrond_wasm::derive_imports!();

const DELEGATION_CLAIM_REWARDS_ENDPOINT: &[u8] = b"claimRewards";

mod dex_proxy {
    elrond_wasm::imports!();

    #[elrond_wasm::proxy]
    pub trait Dex {
        #[payable("*")]
        #[endpoint(swapTokensFixedInput)]
        fn swap_tokens_fixed_input(
            &self,
            #[payment_token] token_in: TokenIdentifier,
            #[payment_amount] amount_in: Self::BigUint,
            token_out: TokenIdentifier,
            amount_out_min: Self::BigUint,
            #[var_args] opt_accept_funds_func: OptionalArg<BoxedBytes>,
        );
    }
}

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct StakingPosition {
    pub liquid_staking_nonce: u64,
    pub prev_pos_id: u64,
    pub next_pos_id: u64,
}

#[elrond_wasm::module]
pub trait StakingRewardsModule:
    crate::multi_transfer::MultiTransferModule + crate::tokens::TokensModule
{
    // endpoints

    // TODO: Ongoing operation pattern
    // TODO: Pause SC while this operation is in progress
    #[endpoint(claimStakingRewards)]
    fn claim_staking_rewards(&self) -> SCResult<()> {
        let current_epoch = self.blockchain().get_block_epoch();
        let last_claim_epoch = self.last_staking_rewards_claim_epoch().get();
        require!(
            current_epoch > last_claim_epoch,
            "Already claimed this epoch"
        );

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        let mut transfers = Vec::new();
        let mut callback_pos_ids = Vec::new();

        let mut pos_id = self.get_first_staking_position_id();
        let mut current_staking_pos = self
            .get_first_staking_position()
            .ok_or("No staking positions available")?;

        loop {
            let sft_nonce = current_staking_pos.liquid_staking_nonce;

            transfers.push(crate::multi_transfer::EsdtTokenPayment {
                token_name: liquid_staking_token_id.clone(),
                token_nonce: sft_nonce,
                amount: self
                    .blockchain()
                    .get_sc_balance(&liquid_staking_token_id, sft_nonce),
                token_type: EsdtTokenType::SemiFungible,
            });
            callback_pos_ids.push(BoxedBytes::from(&pos_id.to_be_bytes()[..]));

            if current_staking_pos.next_pos_id == 0 {
                break;
            }

            pos_id = current_staking_pos.next_pos_id;
            current_staking_pos = self.staking_position(pos_id).get();
        }

        // TODO: Use SC proxy instead of manual call
        let delegation_sc_address = self.delegation_sc_address().get();
        self.multi_transfer_via_async_call(
            &delegation_sc_address,
            &transfers,
            &DELEGATION_CLAIM_REWARDS_ENDPOINT.into(),
            &[],
            &b"claim_staking_rewards_callback"[..].into(),
            &callback_pos_ids,
        );
    }

    // TODO: Convert EGLD to WrapedEgld first (DEX does not convert EGLD directly)
    #[endpoint(convertStakingTokenToStablecoin)]
    fn convert_staking_token_to_stablecoin(&self) -> SCResult<AsyncCall<Self::SendApi>> {
        let current_epoch = self.blockchain().get_block_epoch();
        let last_claim_epoch = self.last_staking_rewards_claim_epoch().get();
        require!(
            last_claim_epoch == current_epoch,
            "Must claim rewards for this epoch first"
        );

        let last_staking_token_convert_epoch = self.last_staking_token_convert_epoch().get();
        require!(
            current_epoch > last_staking_token_convert_epoch,
            "Already converted to stablecoins this epoch"
        );

        let dex_sc_address = self.dex_swap_sc_address().get();

        let staking_token_id = self.staked_token_id().get();
        let staking_token_balance = self.blockchain().get_sc_balance(&staking_token_id, 0);
        let stablecoin_token_id = self.stablecoin_token_id().get();

        Ok(self
            .dex_proxy(dex_sc_address)
            .swap_tokens_fixed_input(
                staking_token_id,
                staking_token_balance,
                stablecoin_token_id,
                Self::BigUint::zero(),
                OptionalArg::Some(b"convert_staking_token_to_stablecoin_callback"[..].into()),
            )
            .async_call())
    }

    // callbacks

    #[payable("*")]
    #[callback]
    fn claim_staking_rewards_callback(
        &self,
        #[call_result] result: AsyncCallResult<()>,
        #[var_args] pos_ids: VarArgs<u64>,
    ) {
        match result {
            AsyncCallResult::Ok(()) => {
                let new_liquid_staking_tokens = self.get_all_esdt_transfers();
                if new_liquid_staking_tokens.len() != pos_ids.len() {
                    return;
                }

                // update liquid staking token nonces
                // needed to know which liquid staking SFT to return on repay
                for (pos_id, new_token) in pos_ids
                    .into_vec()
                    .iter()
                    .zip(new_liquid_staking_tokens.iter())
                {
                    self.staking_position(*pos_id)
                        .update(|pos| pos.liquid_staking_nonce = new_token.token_nonce);
                }

                let current_epoch = self.blockchain().get_block_epoch();
                self.last_staking_rewards_claim_epoch().set(&current_epoch);
            }
            AsyncCallResult::Err(_) => {}
        }
    }

    // Technically, this is not a callback, but its use is simply updating storage after DEX Swap
    #[payable("*")]
    #[endpoint]
    fn convert_staking_token_to_stablecoin_callback(
        &self,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        let caller = self.blockchain().get_caller();
        let dex_swap_sc_address = self.dex_swap_sc_address().get();
        require!(
            caller == dex_swap_sc_address,
            "Only the DEX Swap SC may call this function"
        );

        let current_epoch = self.blockchain().get_block_epoch();
        self.last_staking_token_convert_epoch().set(&current_epoch);
        self.stablecoin_reserves()
            .update(|stablecoin_reserves| *stablecoin_reserves += payment_amount);

        Ok(())
    }

    // private

    fn get_first_staking_position_id(&self) -> u64 {
        self.staking_position(0).get().next_pos_id
    }

    fn get_first_staking_position(&self) -> Option<StakingPosition> {
        let first_id = self.get_first_staking_position_id();
        if first_id != 0 {
            Some(self.staking_position(first_id).get())
        } else {
            None
        }
    }

    fn add_staking_position(&self, liquid_staking_nonce: u64) -> u64 {
        let existing_id = self
            .staking_position_nonce_to_id(liquid_staking_nonce)
            .get();
        if existing_id != 0 {
            return existing_id;
        }

        let prev_last_id = self.last_valid_staking_position_id().get();
        let new_last_id = prev_last_id + 1;

        self.staking_position(prev_last_id)
            .update(|last_pos| last_pos.next_pos_id = new_last_id);
        self.staking_position(new_last_id).set(&StakingPosition {
            liquid_staking_nonce,
            next_pos_id: 0,
            prev_pos_id: prev_last_id,
        });

        self.staking_position_nonce_to_id(liquid_staking_nonce)
            .set(&new_last_id);
        self.last_valid_staking_position_id().set(&new_last_id);

        new_last_id
    }

    fn remove_staking_position(&self, pos_id: u64) {
        if pos_id == 0 {
            return;
        }

        let pos = self.staking_position(pos_id).get();

        // re-connect nodes
        self.staking_position(pos.prev_pos_id)
            .update(|prev_pos| prev_pos.next_pos_id = pos.next_pos_id);

        if pos.next_pos_id != 0 {
            self.staking_position(pos.next_pos_id)
                .update(|next_pos| next_pos.prev_pos_id = pos.prev_pos_id);
        }

        let last_valid_pos_id = self.last_valid_staking_position_id().get();
        if pos_id == last_valid_pos_id {
            self.last_valid_staking_position_id().set(&pos.prev_pos_id)
        }

        self.staking_position(pos_id).clear();
    }

    // proxies

    #[proxy]
    fn dex_proxy(&self, address: Address) -> dex_proxy::Proxy<Self::SendApi>;

    // storage

    #[view(getDelegationScAddress)]
    #[storage_mapper("delegationScAddress")]
    fn delegation_sc_address(&self) -> SingleValueMapper<Self::Storage, Address>;

    #[view(getDexSwapScAddress)]
    #[storage_mapper("dexSwapScAddress")]
    fn dex_swap_sc_address(&self) -> SingleValueMapper<Self::Storage, Address>;

    #[storage_mapper("stakingPosition")]
    fn staking_position(&self, pos_id: u64) -> SingleValueMapper<Self::Storage, StakingPosition>;

    #[storage_mapper("stakingPositionNonceToId")]
    fn staking_position_nonce_to_id(
        &self,
        liquid_staking_nonce: u64,
    ) -> SingleValueMapper<Self::Storage, u64>;

    #[storage_mapper("lastValidStakingPositionId")]
    fn last_valid_staking_position_id(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getLastStakingRewardsClaimEpoch)]
    #[storage_mapper("lastStakingRewardsClaimEpoch")]
    fn last_staking_rewards_claim_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getLastStakingTokenConvertEpoch)]
    #[storage_mapper("lastStakingTokenConvertEpoch")]
    fn last_staking_token_convert_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getLastCalculateRewardsEpoch)]
    #[storage_mapper("lastCalculateRewardsEpoch")]
    fn last_calculate_rewards_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getUnclaimedRewards)]
    #[storage_mapper("unclaimedRewards")]
    fn unclaimed_rewards(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[view(getStablecoinReserves)]
    #[storage_mapper("stablecoinReserves")]
    fn stablecoin_reserves(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;
}