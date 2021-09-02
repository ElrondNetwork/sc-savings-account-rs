elrond_wasm::imports!();

const TICKER_SEPARATOR: u8 = b'-';

pub type AggregatorResultAsMultiResult<BigUint> =
    MultiResult5<u32, BoxedBytes, BoxedBytes, BigUint, u8>;

mod price_aggregator_proxy {
    elrond_wasm::imports!();

    #[elrond_wasm::proxy]
    pub trait PriceAggregator {
        #[view(latestPriceFeedOptional)]
        fn latest_price_feed_optional(
            &self,
            from: BoxedBytes,
            to: BoxedBytes,
        ) -> OptionalResult<super::AggregatorResultAsMultiResult<Self::BigUint>>;
    }
}

pub struct AggregatorResult<BigUint: BigUintApi> {
    pub round_id: u32,
    pub from_token_name: BoxedBytes,
    pub to_token_name: BoxedBytes,
    pub price: BigUint,
    pub decimals: u8,
}

impl<BigUint: BigUintApi> From<AggregatorResultAsMultiResult<BigUint>>
    for AggregatorResult<BigUint>
{
    fn from(multi_result: AggregatorResultAsMultiResult<BigUint>) -> Self {
        let (round_id, from_token_name, to_token_name, price, decimals) = multi_result.into_tuple();

        AggregatorResult {
            round_id,
            from_token_name,
            to_token_name,
            price,
            decimals,
        }
    }
}

#[elrond_wasm::module]
pub trait PriceAggregatorModule {
    #[only_owner]
    #[endpoint(setAggregatorAddress)]
    fn set_aggregator_address(&self, address: Address) -> SCResult<()> {
        self.aggregator_address().set(&address);
        Ok(())
    }

    fn get_price_for_pair(
        &self,
        from: TokenIdentifier,
        to: TokenIdentifier,
    ) -> Option<Self::BigUint> {
        let aggregator_address = self.aggregator_address().get();
        if aggregator_address.is_zero() {
            return None;
        }

        let from_ticker = self.get_token_ticker(from);
        let to_ticker = self.get_token_ticker(to);

        let result: OptionalResult<AggregatorResultAsMultiResult<Self::BigUint>> = self
            .aggregator_proxy(aggregator_address)
            .latest_price_feed_optional(from_ticker, to_ticker)
            .execute_on_dest_context();

        result
            .into_option()
            .map(|multi_result| AggregatorResult::from(multi_result).price)
    }

    fn get_token_ticker(&self, token_id: TokenIdentifier) -> BoxedBytes {
        for (i, char) in token_id.as_esdt_identifier().iter().enumerate() {
            if *char == TICKER_SEPARATOR {
                return token_id.as_esdt_identifier()[..i].into();
            }
        }

        token_id.into_boxed_bytes()
    }

    #[proxy]
    fn aggregator_proxy(&self, address: Address) -> price_aggregator_proxy::Proxy<Self::SendApi>;

    #[view(getAggregatorAddress)]
    #[storage_mapper("aggregator_address")]
    fn aggregator_address(&self) -> SingleValueMapper<Self::Storage, Address>;
}
