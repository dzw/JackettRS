/// Mirror of Jackett.Common/Utils/BuildDate.cs
///
/// C# embedded the build date via an assembly resource (`BuildDate.txt`).
/// Here it is baked at compile time from the env var set by the release
/// pipeline, falling back to the compile timestamp.
pub struct BuildDate;

impl BuildDate {
    pub fn get_build_date() -> String {
        option_env!("JACKETT_BUILD_DATE")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}
