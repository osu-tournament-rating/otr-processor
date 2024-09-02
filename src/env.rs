use dotenv::dotenv;

pub struct EnvironmentVariables {
    pub api_root: String,
    pub privileged_secret: String
}

pub fn get_env() -> EnvironmentVariables {
    dotenv().ok(); // Load environment variables from .env file

    let api_root = std::env::var("API_ROOT").expect("API_ROOT must be set.");
    let privileged_secret = std::env::var("PRIVILEGED_SECRET").expect("PRIVILEGED_SECRET must be set.");

    EnvironmentVariables {
        api_root,
        privileged_secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_get_env() {
        let api_root = "https://otr.stagec.xyz/api/v1".to_string();
        let privileged_secret = "abcd".to_string();

        env::set_var("API_ROOT", &api_root);
        env::set_var("PRIVILEGED_SECRET", &privileged_secret);

        let env = get_env();
        let expected = EnvironmentVariables {
            api_root,
            privileged_secret
        };
    }
}
