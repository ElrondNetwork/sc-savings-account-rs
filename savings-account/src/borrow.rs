elrond_wasm::imports!();

const BASE_PRECISION: u32 = 1000000000;
const SECONDS_IN_YEAR: u32 = 31556926;

#[elrond_wasm::module]
pub trait BorrowModule {
    fn compute_borrow_rate(
        &self,
        r_base: &Self::BigUint,
        r_slope1: &Self::BigUint,
        r_slope2: &Self::BigUint,
        u_optimal: &Self::BigUint,
        u_current: &Self::BigUint,
    ) -> Self::BigUint {
        let bp = Self::BigUint::from(BASE_PRECISION);

        if u_current < u_optimal {
            let utilisation_ratio = &(u_current * r_slope1) / u_optimal;

            r_base + &utilisation_ratio
        } else {
            let denominator = &bp - u_optimal;
            let numerator = &(u_current - u_optimal) * r_slope2;

            (r_base + r_slope1) + numerator / denominator
        }
    }

    fn compute_deposit_rate(
        &self,
        u_current: &Self::BigUint,
        borrow_rate: &Self::BigUint,
        reserve_factor: &Self::BigUint,
    ) -> Self::BigUint {
        let bp = Self::BigUint::from(BASE_PRECISION);
        let loan_ratio = u_current * borrow_rate;
        let deposit_rate = u_current * &loan_ratio * (&bp - reserve_factor);

        deposit_rate / (&bp * &bp * bp)
    }

    fn compute_capital_utilisation(
        &self,
        borrowed_amount: &Self::BigUint,
        total_pool_reserves: &Self::BigUint,
    ) -> Self::BigUint {
        let bp = Self::BigUint::from(BASE_PRECISION);
        &(borrowed_amount * &bp) / total_pool_reserves
    }

    fn compute_debt(
        &self,
        amount: &Self::BigUint,
        time_diff: &Self::BigUint,
        borrow_rate: &Self::BigUint,
    ) -> Self::BigUint {
        let bp = Self::BigUint::from(BASE_PRECISION);
        let secs_year = Self::BigUint::from(SECONDS_IN_YEAR);
        let time_unit_percentage = (time_diff * &bp) / secs_year;
        let debt_percetange = &(&time_unit_percentage * borrow_rate) / &bp;

        (&debt_percetange * amount) / bp
    }

    fn compute_withdrawal_amount(
        &self,
        amount: &Self::BigUint,
        time_diff: &Self::BigUint,
        deposit_rate: &Self::BigUint,
    ) -> Self::BigUint {
        let bp = Self::BigUint::from(BASE_PRECISION);
        let secs_year = Self::BigUint::from(SECONDS_IN_YEAR);
        let percentage = (time_diff * deposit_rate) / secs_year;

        amount + &((&percentage * amount) / bp)
    }
}
