//! Process launch parsing and the Desktop Host's second authorization check.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use next_infra_host_integration::{IntegrationPaths, authorize_mcp_host_launch, clear_user_quit};

use super::lifecycle::LaunchSource;

const BACKGROUND: &str = "--background";
const MCP_SOURCE: &str = "--launch-source=mcp";
const LOGIN_SOURCE: &str = "--launch-source=login";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchAuthorizationError;

pub fn parse_launch_source<I>(arguments: I) -> Result<LaunchSource, LaunchAuthorizationError>
where
    I: IntoIterator<Item = OsString>,
{
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => Ok(LaunchSource::UserInteractive),
        [background, source]
            if background == OsStr::new(BACKGROUND) && source == OsStr::new(MCP_SOURCE) =>
        {
            Ok(LaunchSource::McpAuthorized)
        }
        [background, source]
            if background == OsStr::new(BACKGROUND) && source == OsStr::new(LOGIN_SOURCE) =>
        {
            Ok(LaunchSource::LoginAutostart)
        }
        _ if arguments.iter().any(|argument| {
            argument == OsStr::new(BACKGROUND)
                || argument.to_string_lossy().starts_with("--launch-source=")
        }) =>
        {
            Err(LaunchAuthorizationError)
        }
        _ => Ok(LaunchSource::UserInteractive),
    }
}

pub fn parse_process_arguments<I>(arguments: I) -> Result<LaunchSource, LaunchAuthorizationError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    parse_launch_source(arguments)
}

pub fn authorize_launch(
    source: LaunchSource,
    paths: &IntegrationPaths,
    current_app_bundle: &Path,
) -> Result<(), LaunchAuthorizationError> {
    match source {
        LaunchSource::UserInteractive | LaunchSource::LoginAutostart => {
            clear_user_quit(paths).map_err(|_| LaunchAuthorizationError)
        }
        LaunchSource::McpAuthorized => authorize_mcp_host_launch(paths, current_app_bundle)
            .map(|_| ())
            .map_err(|_| LaunchAuthorizationError),
    }
}

pub fn app_bundle_from_executable(executable: &Path) -> Option<&Path> {
    executable
        .ancestors()
        .find(|ancestor| ancestor.extension() == Some(OsStr::new("app")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_host_integration::{UserQuitInspection, inspect_user_quit, persist_user_quit};
    use tempfile::TempDir;

    #[test]
    fn only_fixed_background_tuples_select_privileged_sources() {
        assert_eq!(
            parse_launch_source(Vec::<OsString>::new()),
            Ok(LaunchSource::UserInteractive)
        );
        assert_eq!(
            parse_launch_source([BACKGROUND.into(), MCP_SOURCE.into()]),
            Ok(LaunchSource::McpAuthorized)
        );
        assert_eq!(
            parse_launch_source([BACKGROUND.into(), LOGIN_SOURCE.into()]),
            Ok(LaunchSource::LoginAutostart)
        );
        assert!(parse_launch_source([MCP_SOURCE.into()]).is_err());
        assert!(
            parse_launch_source([BACKGROUND.into(), MCP_SOURCE.into(), "extra".into()]).is_err()
        );
        assert_eq!(
            parse_launch_source(["--ordinary-user-argument".into()]),
            Ok(LaunchSource::UserInteractive)
        );
        assert_eq!(
            parse_process_arguments([
                "/Applications/Next Infra.app/Contents/MacOS/next-infra".into(),
                BACKGROUND.into(),
                MCP_SOURCE.into(),
            ]),
            Ok(LaunchSource::McpAuthorized)
        );
    }

    #[test]
    fn app_bundle_is_derived_only_from_an_app_ancestor() {
        assert_eq!(
            app_bundle_from_executable(Path::new(
                "/Users/fixture/Applications/Next Infra.app/Contents/MacOS/next-infra"
            )),
            Some(Path::new("/Users/fixture/Applications/Next Infra.app"))
        );
        assert_eq!(
            app_bundle_from_executable(Path::new("/tmp/next-infra")),
            None
        );
    }

    #[test]
    fn only_interactive_and_login_launches_clear_user_quit() {
        for source in [LaunchSource::UserInteractive, LaunchSource::LoginAutostart] {
            let home = TempDir::new().unwrap();
            let paths = IntegrationPaths::from_home(home.path());
            persist_user_quit(&paths).unwrap();
            authorize_launch(source, &paths, Path::new("/unused.app")).unwrap();
            assert_eq!(inspect_user_quit(&paths), UserQuitInspection::Clear);
        }

        let home = TempDir::new().unwrap();
        let paths = IntegrationPaths::from_home(home.path());
        persist_user_quit(&paths).unwrap();
        assert!(
            authorize_launch(
                LaunchSource::McpAuthorized,
                &paths,
                Path::new("/unused.app")
            )
            .is_err()
        );
        assert_eq!(inspect_user_quit(&paths), UserQuitInspection::Suppressed);
    }
}
