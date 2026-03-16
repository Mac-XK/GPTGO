pub const OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const OAUTH_AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const OAUTH_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const OAUTH_SCOPE: &str = "openid email profile offline_access";

pub const SENTINEL_URL: &str = "https://sentinel.openai.com/backend-api/sentinel/req";
pub const SIGNUP_URL: &str = "https://auth.openai.com/api/accounts/authorize/continue";
pub const REGISTER_URL: &str = "https://auth.openai.com/api/accounts/user/register";
pub const SEND_OTP_URL: &str = "https://auth.openai.com/api/accounts/email-otp/send";
pub const VALIDATE_OTP_URL: &str = "https://auth.openai.com/api/accounts/email-otp/validate";
pub const CREATE_ACCOUNT_URL: &str = "https://auth.openai.com/api/accounts/create_account";
pub const SELECT_WORKSPACE_URL: &str = "https://auth.openai.com/api/accounts/workspace/select";

pub const PASSWORD_CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
