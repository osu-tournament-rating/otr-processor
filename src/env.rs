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
