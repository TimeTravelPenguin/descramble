type ValidatorFn<T, E> = dyn Fn(&str) -> Result<T, E>;

pub(super) struct ValidatedBinding<T, E>
where
    T: Clone + ToString,
{
    raw_value: String,
    validated_value: T,
    is_valid: bool,
    only_update_on_valid: bool,
    validator: Box<ValidatorFn<T, E>>,
}

impl<T, E> ValidatedBinding<T, E>
where
    T: Clone + ToString,
{
    pub fn new<F>(initial_value: T, validator: F) -> Self
    where
        F: Fn(&str) -> Result<T, E> + 'static,
    {
        let validated_value = initial_value.clone();
        let raw_value = initial_value.to_string();

        Self {
            raw_value,
            validated_value,
            is_valid: true,
            only_update_on_valid: false,
            validator: Box::new(validator),
        }
    }

    pub fn only_update_raw_on_valid(mut self, only_update_on_valid: bool) -> Self {
        self.only_update_on_valid = only_update_on_valid;
        self
    }

    pub fn update(&mut self, new_value: &str) -> Result<T, E> {
        let result = match (self.validator)(new_value) {
            Ok(validated) => {
                self.validated_value = validated.clone();
                Ok(validated)
            }
            Err(e) => Err(e),
        };

        if result.is_ok() || !self.only_update_on_valid {
            self.raw_value = new_value.to_string();
        }

        self.is_valid = result.is_ok();

        result
    }

    pub fn set(mut self, new_value: T) -> Result<Self, E> {
        let raw_value = new_value.to_string();

        self.raw_value = raw_value;
        self.validated_value = new_value;
        self.is_valid = true;

        Ok(self)
    }

    pub fn get_raw(&self) -> &str {
        &self.raw_value
    }

    pub fn get_validated(&self) -> &T {
        &self.validated_value
    }

    pub fn is_valid(&self) -> bool {
        self.is_valid
    }
}
