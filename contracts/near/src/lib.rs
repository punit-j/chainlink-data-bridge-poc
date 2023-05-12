use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::env::{current_account_id, block_height};
use near_sdk::serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use near_sdk::Promise;
use near_sdk::{
    env, ext_contract, near_bindgen, require, AccountId, BorshStorageKey, PanicOnDefault, PromiseOrValue,
};

pub type EthAddress = [u8; 20];
pub const TGAS: near_sdk::Gas = near_sdk::Gas::ONE_TERA;
pub const NO_DEPOSIT: u128 = 0;


#[derive(BorshSerialize, BorshStorageKey)]
enum StorageKey {
   Symbol
}

#[ext_contract(ext_prover)]
pub trait Prover {
    #[result_serializer(borsh)]
    fn verify_log_entry(
        &self,
        #[serializer(borsh)] log_index: u64,
        #[serializer(borsh)] log_entry_data: Vec<u8>,
        #[serializer(borsh)] receipt_index: u64,
        #[serializer(borsh)] receipt_data: Vec<u8>,
        #[serializer(borsh)] header_data: Vec<u8>,
        #[serializer(borsh)] proof: Vec<Vec<u8>>,
        #[serializer(borsh)] skip_bridge_call: bool,
    ) -> bool;

    #[result_serializer(borsh)]
    fn verify_storage_proof(
        &self,
        #[serializer(borsh)] header_data: Vec<u8>,
        #[serializer(borsh)] account_proof: Vec<Vec<u8>>, // account proof
        #[serializer(borsh)] contract_address: Vec<u8>,   // eth address
        #[serializer(borsh)] expected_account_state: Vec<u8>, // encoded account state
        #[serializer(borsh)] storage_key_hash: Vec<u8>,   // keccak256 of storage key
        #[serializer(borsh)] storage_proof: Vec<Vec<u8>>, // storage proof
        #[serializer(borsh)] expected_storage_value: Vec<u8>, // storage value
        #[serializer(borsh)] min_header_height: Option<u64>,
        #[serializer(borsh)] max_header_height: Option<u64>,
        #[serializer(borsh)] skip_bridge_call: bool,
    ) -> PromiseOrValue<bool>;
}

#[ext_contract(ext_self)]
trait ChainLinkBridgeInterface {
    fn data_proof_callback(
        &mut self,
        #[callback]
        #[serializer(borsh)]
        verification_success: bool,
        #[serializer(borsh)] symbol: String,
        #[serializer(borsh)] value: u128,
        #[serializer(borsh)] eth_height: u64,
    );
}

#[derive(
    BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize, PartialEq,
)]
pub struct DataProof{
    header_data: Vec<u8>,
    account_proof: Vec<Vec<u8>>, // account proof
    account_state: Vec<u8>,      // rlp encoded account state
    storage_proof: Vec<Vec<u8>>, // storage proof
    storage_key_hash: Vec<u8>,   // keccak256 of storage key
    value: Vec<u8>,      // storage value
    eth_height: u64,
}

#[derive(
    BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize, PartialEq,
)]
pub struct PriceFeed{
    latest_price: u128,
    added_at: u64,
    eth_height: u64,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct ChainLinkBridge{
    symbol_to_pricefeed_address: LookupMap<String, EthAddress>,
    latest_price: LookupMap<String, PriceFeed>,
    prover_account: AccountId,
    min_block_delay_near: u64,
    min_block_delay_eth: u64,
}

#[near_bindgen]
impl ChainLinkBridge {
    #[init]
    #[private]
    pub fn new(
        prover_account: AccountId,
        min_block_delay_near: u64,
        min_block_delay_eth: u64,
    ) -> Self {
        require!(!env::state_exists(), "Already initialized");
        let contract = Self {
            symbol_to_pricefeed_address: LookupMap::new(StorageKey::Symbol),
            latest_price: LookupMap::new(StorageKey::Symbol),
            prover_account,
            min_block_delay_near,
            min_block_delay_eth
        };
        contract
    }

    pub fn add_feed_data(&mut self, symbol: String, data_proof: near_sdk::json_types::Base64VecU8) -> Promise{

        let data_proof = DataProof::try_from_slice(&data_proof.0)
        .unwrap_or_else(|_| env::panic_str("Invalid borsh format of the `DataProof`"));

        let feed_address = self.symbol_to_pricefeed_address.get(&symbol).unwrap_or_else(|| {
            panic!("Price Feed not registered for {} symbol", symbol)
        });

        // let previous_data = self.latest_price.get(&symbol).unwrap_or_else(|| {
        //     panic!("Price not registered for {} symbol", symbol)
        // });

        // require!(block_height() - previous_data.added_at >= self.min_block_delay_near, "Should cross min block delay for near");
        // require!(data_proof.eth_height - previous_data.eth_height >= self.min_block_delay_eth, "Should cross min block delay for eth");
        let mut value_vec:Vec<u8> = vec![156];
        value_vec.extend(data_proof.value.clone());
        ext_prover::ext(self.prover_account.clone())
            .with_static_gas(tera_gas(50))
            .with_attached_deposit(NO_DEPOSIT)
            .verify_storage_proof(
                data_proof.header_data.clone(),
                data_proof.account_proof.clone(),
                feed_address.to_vec(),
                data_proof.account_state.clone(),
                data_proof.storage_key_hash.clone(),
                data_proof.storage_proof.clone(),
                value_vec,
                Some(data_proof.eth_height.clone()),
                None,
                true,
            )
            .then(
                ext_self::ext(current_account_id())
                    .with_static_gas(tera_gas(50))
                    .with_attached_deposit(NO_DEPOSIT)
                    .data_proof_callback(
                        symbol,
                        get_value_from_proof(&data_proof.value),
                        data_proof.eth_height
                    ),
            )
    }

    #[private]
    pub fn data_proof_callback(
        &mut self,
        #[callback]
        #[serializer(borsh)]
        verification_success: bool,
        #[serializer(borsh)] symbol: String,
        #[serializer(borsh)] value: u128,
        #[serializer(borsh)] eth_height: u64,
    ) {
        require!(
            verification_success,
            format!("Verification failed for data proof")
        );

        self.latest_price.insert(&symbol, &PriceFeed { latest_price: u128::from(value) , added_at: block_height(), eth_height: eth_height});
    }

    //adds new price feeds with corresponding chainlink address, eg BTC/USD
    pub fn add_price_feed(&mut self, symbol: String, pricefeed_address: String) {
        self.symbol_to_pricefeed_address.insert(&symbol, &get_eth_address(pricefeed_address));
        // let price_feed = PriceFeed { latest_price: 0, added_at: 0, eth_height: 0 };
        // self.latest_price.insert(&"ETH/USD".to_string(), &price_feed);
    }

    pub fn get_latest_price(&self, symbol:String) -> PriceFeed{
        self.latest_price.get(&symbol).unwrap()
    }

    pub fn get_symbol_to_pricefeed_address(&self, symbol: String) -> String{
        let symbol_price_feed_address = self.symbol_to_pricefeed_address.get(&symbol).unwrap_or_else(|| near_sdk::env::panic_str("unable to get price feed address in hex"));
        hex::encode(symbol_price_feed_address)
    }
}

pub fn tera_gas(gas: u64) -> near_sdk::Gas {
    TGAS * gas
}

pub fn get_eth_address(address: String) -> EthAddress {
    let data = hex::decode(address)
        .unwrap_or_else(|_| near_sdk::env::panic_str("address should be a valid hex string."));
    require!(data.len() == 20, "address should be 20 bytes long");
    data.try_into().unwrap_or_else(|_| near_sdk::env::panic_str("cannot unwrap"))
}

pub fn get_value_from_proof(value: &Vec<u8>) -> u128{
    let length = value.len();
    let bytes_relevant: [u8; 16] = value[length-16..length].try_into().expect("slice with incorrect length");
    u128::from_be_bytes(bytes_relevant)
}

#[cfg(test)]
mod tests{
    use super::*;
    use near_sdk::test_utils::{accounts};

    #[test]
    fn test_add() {
        let mut contract = ChainLinkBridge::new(accounts(1), 0, 0);
        contract.add_price_feed("ETH/USD".to_string(), "37bC7498f4FF12C19678ee8fE19d713b87F6a9e6".to_string())
    }


    #[test]
    fn test_data_proof_callback() {
        let value: Vec<u8> = hex::decode("9c644b6de7000000000000000000000000000000000000002c944002c0").unwrap();
        let data_proof = DataProof{
            header_data: vec![],
            account_proof:vec![vec![]],
            account_state: vec![],
            storage_proof: vec![vec![]],
            storage_key_hash: vec![],
            value: value,
            eth_height: 100
        };
        let mut contract = ChainLinkBridge::new(accounts(1), 0, 0);
        contract.data_proof_callback(true, "ETH/USD".to_string(), get_value_from_proof(&data_proof.value), data_proof.eth_height);
        println!("{:?}", contract.get_latest_price("ETH/USD".to_string()));
    }

    #[test]
    fn test_get_value_from_proof() {
        let value: Vec<u8> = hex::decode("644b6de7000000000000000000000000000000000000002c944002c0").unwrap();
        let data_proof = DataProof{
            header_data: vec![],
            account_proof:vec![vec![]],
            account_state: vec![],
            storage_proof: vec![vec![]],
            storage_key_hash: vec![],
            value: value,
            eth_height: 100
        };
        println!("{:?}", get_value_from_proof(&data_proof.value));
    }
}