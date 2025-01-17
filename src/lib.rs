/// A library to process Server.toml files

#[macro_use] mod error;
mod core;
mod actix;

use actix_http::{body::MessageBody, KeepAlive as ActixKeepAlive, Request, Response};
use actix_service::{IntoServiceFactory, ServiceFactory};
use actix_web::{Error as WebError, HttpServer};
use actix_web::dev::{AppConfig, Service};
pub use crate::core::Parse;
pub use crate::actix::*;
pub use crate::error::{AtError, AtResult};
use serde_derive::Deserialize;
use std::env::{self, VarError};
use std::io::{Read, Write};
use std::fmt::Debug;
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash)]
#[serde(bound = "A: serde::de::Deserialize<'de>")]
pub struct BasicSettings<A> {
    pub actix: ActixSettings,
    pub application: A,
}

pub type Settings = BasicSettings::<NoSettings>;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash)]
pub struct NoSettings {/* NOTE: **DO NOT** turn this into a unit struct */}


impl<A> BasicSettings<A>
where A: for<'de> serde::de::Deserialize<'de> {

    /// NOTE **DO NOT** mess with the ordering of the tables in this template.
    ///      Especially the `[application]` table needs to be last in order
    ///      for some tests to keep working.
    pub(crate) const DEFAULT_TOML_TEMPLATE: &'static str = r#"
[actix]
# For more info, see: https://docs.rs/actix-web/3.1.0/actix_web/struct.HttpServer.html.

hosts = [
    ["0.0.0.0", 9000]      # This should work for both development and deployment...
    #                      # ... but other entries are possible, as well.
]
mode = "development"       # Either "development" or "production".
enable-compression = true  # Toggle compression middleware.
enable-log = true          # Toggle logging middleware.

# The number of workers that the server should start.
# By default the number of available logical cpu cores is used.
# Takes a string value: Either "default", or an integer N > 0 e.g. "6".
num-workers = "default"

# The maximum number of pending connections.  This refers to the number of clients
# that can be waiting to be served.  Exceeding this number results in the client
# getting an error when attempting to connect.  It should only affect servers under
# significant load.  Generally set in the 64-2048 range.  The default value is 2048.
# Takes a string value: Either "default", or an integer N > 0 e.g. "6".
backlog = "default"

# Sets the maximum per-worker number of concurrent connections.  All socket listeners
# will stop accepting connections when this limit is reached for each worker.
# By default max connections is set to a 25k.
# Takes a string value: Either "default", or an integer N > 0 e.g. "6".
max-connections = "default"

# Sets the maximum per-worker concurrent connection establish process.  All listeners
# will stop accepting connections when this limit is reached. It can be used to limit
# the global TLS CPU usage.  By default max connections is set to a 256.
# Takes a string value: Either "default", or an integer N > 0 e.g. "6".
max-connection-rate = "default"

# Set server keep-alive setting.  By default keep alive is set to 5 seconds.
# Takes a string value: Either "default", "disabled", "os",
# or a string of the format "N seconds" where N is an integer > 0 e.g. "6 seconds".
keep-alive = "default"

# Set server client timeout in milliseconds for first request.  Defines a timeout
# for reading client request header. If a client does not transmit the entire set of
# headers within this time, the request is terminated with the 408 (Request Time-out)
# error.  To disable timeout, set the value to 0.
# By default client timeout is set to 5000 milliseconds.
# Takes a string value: Either "default", or a string of the format "N milliseconds"
# where N is an integer > 0 e.g. "6 milliseconds".
client-timeout = "default"

# Set server connection shutdown timeout in milliseconds.  Defines a timeout for
# shutdown connection. If a shutdown procedure does not complete within this time,
# the request is dropped.  To disable timeout set value to 0.
# By default client timeout is set to 5000 milliseconds.
# Takes a string value: Either "default", or a string of the format "N milliseconds"
# where N is an integer > 0 e.g. "6 milliseconds".
client-shutdown = "default"

# Timeout for graceful workers shutdown. After receiving a stop signal, workers have
# this much time to finish serving requests. Workers still alive after the timeout
# are force dropped.  By default shutdown timeout sets to 30 seconds.
# Takes a string value: Either "default", or a string of the format "N seconds"
# where N is an integer > 0 e.g. "6 seconds".
shutdown-timeout = "default"

[actix.ssl] # SSL is disabled by default because the certs don't exist
enabled = false
certificate = "path/to/cert/cert.pem"
private-key = "path/to/cert/key.pem"

# The `application` table be used to express application-specific settings.
# See the `README.md` file for more details on how to use this.
[application]
"#;

    /// Parse an instance of `Self` from a `TOML` file located at `filepath`.
    /// If the file doesn't exist, it is generated from the default `TOML`
    /// template, after which the newly generated file is read in and parsed.
    pub fn parse_toml<P>(filepath: P) -> AtResult<Self>
    where P: AsRef<Path> {
        let filepath = filepath.as_ref();
        if !filepath.exists() { Self::write_toml_file(filepath)?; }
        let mut f = File::open(filepath)?;
        let mut contents = String::with_capacity(f.metadata()?.len() as usize);
        f.read_to_string(&mut contents)?;
        Ok(toml::from_str::<Self>(&contents)?)
    }

    /// Parse an instance of `Self` straight from the default `TOML` template.
    pub fn from_default_template() -> AtResult<Self> {
        Self::from_template(Self::DEFAULT_TOML_TEMPLATE)
    }

    /// Parse an instance of `Self` straight from the default `TOML` template.
    pub fn from_template(template: &str) -> AtResult<Self> {
        Ok(toml::from_str::<Self>(template)?)
    }

    /// Write the default `TOML` template to a new file, to be located
    /// at `filepath`.  Return a `Error::FileExists(_)` error if a
    /// file already exists at that location.
    pub fn write_toml_file<P>(filepath: P) -> AtResult<()>
    where P: AsRef<Path> {
        let filepath = filepath.as_ref();
        let contents = Self::DEFAULT_TOML_TEMPLATE.trim();
        if filepath.exists() {
            return Err(AtError::FileExists(filepath.to_path_buf()));
        }
        let mut file = File::create(filepath)?;
        file.write_all(contents.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    pub fn override_field<F, V>(
        field: &mut F,
        value: V
    ) -> AtResult<()>
    where F: Parse,
          V: AsRef<str> {
        *field = F::parse(value.as_ref())?;
        Ok(())
    }

    pub fn override_field_with_env_var<F, N>(
        field: &mut F,
        var_name: N,
    ) -> AtResult<()>
    where F: Parse,
          N: AsRef<str> {
        match env::var(var_name.as_ref()) {
            Err(VarError::NotPresent) => Ok((/*NOP*/)),
            Err(var_error) => Err(AtError::from(var_error)),
            Ok(value) => Self::override_field(field, value),
        }
    }
}



pub trait ApplySettings {
    #[must_use]
    /// Apply a [`BasicSettings`] value to `self`.
    ///
    /// [`BasicSettings`]: ./struct.BasicSettings.html
    fn apply_settings<A>(self, settings: &BasicSettings<A>) -> Self
    where A: for<'de> serde::de::Deserialize<'de>;
}

impl<F, I, S, B> ApplySettings for HttpServer<F, I, S, B>
where
    F: Fn() -> I + Send + Clone + 'static,
    I: IntoServiceFactory<S>,
    S: ServiceFactory<Config = AppConfig, Request = Request>,
    S::Error: Into<WebError> + 'static,
    S::InitError: Debug,
    S::Response: Into<Response<B>> + 'static,
    <S::Service as Service>::Future: 'static,
    B: MessageBody + 'static
{
    fn apply_settings<A>(mut self, settings: &BasicSettings<A>) -> Self
    where A: for<'de> serde::de::Deserialize<'de> {
        if settings.actix.ssl.enabled {
            // for Address { host, port } in &settings.actix.hosts {
            //     self = self.bind(format!("{}:{}", host, port))
            //         .unwrap(/*TODO*/);
            // }
            todo!("[ApplySettings] SSL support has not been implemented yet.");
        } else {
            for Address { host, port } in &settings.actix.hosts {
                self = self.bind(format!("{}:{}", host, port))
                    .unwrap(/*TODO*/);
            }
        }
        self = match settings.actix.num_workers {
            NumWorkers::Default   => self,
            NumWorkers::Manual(n) => self.workers(n),
        };
        self = match settings.actix.backlog {
            Backlog::Default   => self,
            Backlog::Manual(n) => self.backlog(n as i32),
        };
        self = match settings.actix.max_connections {
            MaxConnections::Default   => self,
            MaxConnections::Manual(n) => self.max_connections(n),
        };
        self = match settings.actix.max_connection_rate {
            MaxConnectionRate::Default   => self,
            MaxConnectionRate::Manual(n) => self.max_connection_rate(n),
        };
        self = match settings.actix.keep_alive {
            KeepAlive::Default    => self,
            KeepAlive::Disabled   => self.keep_alive(ActixKeepAlive::Disabled),
            KeepAlive::Os         => self.keep_alive(ActixKeepAlive::Os),
            KeepAlive::Seconds(n) => self.keep_alive(n),
        };
        self = match settings.actix.client_timeout {
            Timeout::Default         => self,
            Timeout::Milliseconds(n) => self.client_timeout(n as u64),
            Timeout::Seconds(n)      => self.client_timeout(n as u64 * 1000),
        };
        self = match settings.actix.client_shutdown {
            Timeout::Default         => self,
            Timeout::Milliseconds(n) => self.client_shutdown(n as u64),
            Timeout::Seconds(n)      => self.client_shutdown(n as u64 * 1000),
        };
        self = match settings.actix.shutdown_timeout {
            Timeout::Default         => self,
            Timeout::Milliseconds(_) => self.shutdown_timeout(1),
            Timeout::Seconds(n)      => self.shutdown_timeout(n as u64),
        };
        self
    }
}



#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]

    use actix_web::{App, HttpServer};
    use crate::{ApplySettings, AtResult, BasicSettings, Settings};
    use crate::actix::*; // used for value construction in assertions
    use serde::Deserialize;
    use std::path::Path;

    #[test]
    fn apply_settings() -> AtResult<()> {
        let settings = Settings::parse_toml("Server.toml")?;
        let _ = HttpServer::new(|| { App::new() })
            .apply_settings(&settings);
        Ok(())
    }

    #[test]
    fn override_field__hosts() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.hosts, vec![
            Address { host: "0.0.0.0".into(),   port: 9000 },
        ]);
        Settings::override_field(&mut settings.actix.hosts, r#"[
            ["0.0.0.0",   1234],
            ["localhost", 2345]
        ]"#)?;
        assert_eq!(settings.actix.hosts, vec![
            Address { host: "0.0.0.0".into(),   port: 1234 },
            Address { host: "localhost".into(), port: 2345 },
        ]);
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__hosts() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.hosts, vec![
            Address { host: "0.0.0.0".into(),   port: 9000 },
        ]);
        std::env::set_var("OVERRIDE__HOSTS", r#"[
            ["0.0.0.0",   1234],
            ["localhost", 2345]
        ]"#);
        Settings::override_field_with_env_var(
            &mut settings.actix.hosts, "OVERRIDE__HOSTS"
        )?;
        assert_eq!(settings.actix.hosts, vec![
            Address { host: "0.0.0.0".into(),   port: 1234 },
            Address { host: "localhost".into(), port: 2345 },
        ]);
        Ok(())
    }

    #[test]
    fn override_field__mode() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.mode, Mode::Development);
        Settings::override_field(&mut settings.actix.mode, "production")?;
        assert_eq!(settings.actix.mode, Mode::Production);
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__mode() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.mode, Mode::Development);
        std::env::set_var("OVERRIDE__MODE", "production");
        Settings::override_field_with_env_var(
            &mut settings.actix.mode, "OVERRIDE__MODE"
        )?;
        assert_eq!(settings.actix.mode, Mode::Production);
        Ok(())
    }

    #[test]
    fn override_field__enable_compression() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert!(settings.actix.enable_compression);
        Settings::override_field(&mut settings.actix.enable_compression, "false")?;
        assert!(!settings.actix.enable_compression);
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__enable_compression() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert!(settings.actix.enable_compression);
        std::env::set_var("OVERRIDE__ENABLE_COMPRESSION", "false");
        Settings::override_field_with_env_var(
            &mut settings.actix.enable_compression, "OVERRIDE__ENABLE_COMPRESSION"
        )?;
        assert!(!settings.actix.enable_compression);
        Ok(())
    }

    #[test]
    fn override_field__enable_log() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert!(settings.actix.enable_log);
        Settings::override_field(&mut settings.actix.enable_log, "false")?;
        assert!(!settings.actix.enable_log);
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__enable_log() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert!(settings.actix.enable_log);
        std::env::set_var("OVERRIDE__ENABLE_LOG", "false");
        Settings::override_field_with_env_var(
            &mut settings.actix.enable_log, "OVERRIDE__ENABLE_LOG"
        )?;
        assert!(!settings.actix.enable_log);
        Ok(())
    }

    #[test]
    fn override_field__num_workers() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.num_workers, NumWorkers::Default);
        Settings::override_field(&mut settings.actix.num_workers, "42")?;
        assert_eq!(settings.actix.num_workers, NumWorkers::Manual(42));
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__num_workers() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.num_workers, NumWorkers::Default);
        std::env::set_var("OVERRIDE__NUM_WORKERS", "42");
        Settings::override_field_with_env_var(
            &mut settings.actix.num_workers, "OVERRIDE__NUM_WORKERS"
        )?;
        assert_eq!(settings.actix.num_workers, NumWorkers::Manual(42));
        Ok(())
    }

    #[test]
    fn override_field__backlog() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.backlog, Backlog::Default);
        Settings::override_field(&mut settings.actix.backlog, "42")?;
        assert_eq!(settings.actix.backlog, Backlog::Manual(42));
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__backlog() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.backlog, Backlog::Default);
        std::env::set_var("OVERRIDE__BACKLOG", "42");
        Settings::override_field_with_env_var(
            &mut settings.actix.backlog, "OVERRIDE__BACKLOG"
        )?;
        assert_eq!(settings.actix.backlog, Backlog::Manual(42));
        Ok(())
    }

    #[test]
    fn override_field__max_connections() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.max_connections, MaxConnections::Default);
        Settings::override_field(&mut settings.actix.max_connections, "42")?;
        assert_eq!(settings.actix.max_connections, MaxConnections::Manual(42));
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__max_connections() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.max_connections, MaxConnections::Default);
        std::env::set_var("OVERRIDE__MAX_CONNECTIONS", "42");
        Settings::override_field_with_env_var(
            &mut settings.actix.max_connections, "OVERRIDE__MAX_CONNECTIONS"
        )?;
        assert_eq!(settings.actix.max_connections, MaxConnections::Manual(42));
        Ok(())
    }

    #[test]
    fn override_field__max_connection_rate() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.max_connection_rate, MaxConnectionRate::Default);
        Settings::override_field(&mut settings.actix.max_connection_rate, "42")?;
        assert_eq!(settings.actix.max_connection_rate, MaxConnectionRate::Manual(42));
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__max_connection_rate() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.max_connection_rate, MaxConnectionRate::Default);
        std::env::set_var("OVERRIDE__MAX_CONNECTION_RATE", "42");
        Settings::override_field_with_env_var(
            &mut settings.actix.max_connection_rate, "OVERRIDE__MAX_CONNECTION_RATE"
        )?;
        assert_eq!(settings.actix.max_connection_rate, MaxConnectionRate::Manual(42));
        Ok(())
    }

    #[test]
    fn override_field__keep_alive() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.keep_alive, KeepAlive::Default);
        Settings::override_field(&mut settings.actix.keep_alive, "42 seconds")?;
        assert_eq!(settings.actix.keep_alive, KeepAlive::Seconds(42));
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__keep_alive() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.keep_alive, KeepAlive::Default);
        std::env::set_var("OVERRIDE__KEEP_ALIVE", "42 seconds");
        Settings::override_field_with_env_var(
            &mut settings.actix.keep_alive, "OVERRIDE__KEEP_ALIVE"
        )?;
        assert_eq!(settings.actix.keep_alive, KeepAlive::Seconds(42));
        Ok(())
    }

    #[test]
    fn override_field__client_timeout() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.client_timeout, Timeout::Default);
        Settings::override_field(&mut settings.actix.client_timeout, "42 seconds")?;
        assert_eq!(settings.actix.client_timeout, Timeout::Seconds(42));
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__client_timeout() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.client_timeout, Timeout::Default);
        std::env::set_var("OVERRIDE__CLIENT_TIMEOUT", "42 seconds");
        Settings::override_field_with_env_var(
            &mut settings.actix.client_timeout, "OVERRIDE__CLIENT_TIMEOUT"
        )?;
        assert_eq!(settings.actix.client_timeout, Timeout::Seconds(42));
        Ok(())
    }

    #[test]
    fn override_field__client_shutdown() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.client_shutdown, Timeout::Default);
        Settings::override_field(&mut settings.actix.client_shutdown, "42 seconds")?;
        assert_eq!(settings.actix.client_shutdown, Timeout::Seconds(42));
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__client_shutdown() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.client_shutdown, Timeout::Default);
        std::env::set_var("OVERRIDE__CLIENT_SHUTDOWN", "42 seconds");
        Settings::override_field_with_env_var(
            &mut settings.actix.client_shutdown, "OVERRIDE__CLIENT_SHUTDOWN"
        )?;
        assert_eq!(settings.actix.client_shutdown, Timeout::Seconds(42));
        Ok(())
    }

    #[test]
    fn override_field__shutdown_timeout() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.shutdown_timeout, Timeout::Default);
        Settings::override_field(&mut settings.actix.shutdown_timeout, "42 seconds")?;
        assert_eq!(settings.actix.shutdown_timeout, Timeout::Seconds(42));
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__shutdown_timeout() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.shutdown_timeout, Timeout::Default);
        std::env::set_var("OVERRIDE__SHUTDOWN_TIMEOUT", "42 seconds");
        Settings::override_field_with_env_var(
            &mut settings.actix.shutdown_timeout, "OVERRIDE__SHUTDOWN_TIMEOUT"
        )?;
        assert_eq!(settings.actix.shutdown_timeout, Timeout::Seconds(42));
        Ok(())
    }



    #[test]
    fn override_field__ssl__enabled() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert!(!settings.actix.ssl.enabled);
        Settings::override_field(&mut settings.actix.ssl.enabled, "true")?;
        assert!(settings.actix.ssl.enabled);
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__ssl__enabled() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert!(!settings.actix.ssl.enabled);
        std::env::set_var("OVERRIDE__SSL_ENABLED", "true");
        Settings::override_field_with_env_var(
            &mut settings.actix.ssl.enabled, "OVERRIDE__SSL_ENABLED"
        )?;
        assert!(settings.actix.ssl.enabled);
        Ok(())
    }

    #[test]
    fn override_field__ssl__certificate() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.ssl.certificate, Path::new("path/to/cert/cert.pem"));
        Settings::override_field(
            &mut settings.actix.ssl.certificate, "/overridden/path/to/cert/cert.pem"
        )?;
        assert_eq!(
            settings.actix.ssl.certificate, Path::new("/overridden/path/to/cert/cert.pem")
        );
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__ssl__certificate() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.ssl.certificate, Path::new("path/to/cert/cert.pem"));
        std::env::set_var(
            "OVERRIDE__SSL_CERTIFICATE", "/overridden/path/to/cert/cert.pem"
        );
        Settings::override_field_with_env_var(
            &mut settings.actix.ssl.certificate, "OVERRIDE__SSL_CERTIFICATE"
        )?;
        assert_eq!(
            settings.actix.ssl.certificate,
            Path::new("/overridden/path/to/cert/cert.pem")
        );
        Ok(())
    }

    #[test]
    fn override_field__ssl__private_key() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.ssl.private_key, Path::new("path/to/cert/key.pem"));
        Settings::override_field(
            &mut settings.actix.ssl.private_key, "/overridden/path/to/cert/key.pem"
        )?;
        assert_eq!(
            settings.actix.ssl.private_key, Path::new("/overridden/path/to/cert/key.pem")
        );
        Ok(())
    }

    #[test]
    fn override_field_with_env_var__ssl__private_key() -> AtResult<()> {
        let mut settings = Settings::from_default_template()?;
        assert_eq!(settings.actix.ssl.private_key, Path::new("path/to/cert/key.pem"));
        std::env::set_var(
            "OVERRIDE__SSL_PRIVATE_KEY", "/overridden/path/to/cert/key.pem"
        );
        Settings::override_field_with_env_var(
            &mut settings.actix.ssl.private_key, "OVERRIDE__SSL_PRIVATE_KEY"
        )?;
        assert_eq!(
            settings.actix.ssl.private_key,
            Path::new("/overridden/path/to/cert/key.pem")
        );
        Ok(())
    }

    #[test]
    fn override_extended_field_with_custom_type() -> AtResult<()> {
        #[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
        struct NestedSetting {
            foo: String,
            bar: bool,
        }
        #[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
        struct AppSettings {
            #[serde(rename = "example-name")]
            example_name: String,
            #[serde(rename = "nested-field")]
            nested_field: NestedSetting,
        }
        type CustomSettings = BasicSettings<AppSettings>;
        let mut settings = CustomSettings::from_template(&(
            CustomSettings::DEFAULT_TOML_TEMPLATE.to_string()
                // NOTE: Add these entries to the `[application]` table:
                + "\nexample-name = \"example value\""
                + "\nnested-field = { foo = \"foo\", bar = false }"
        ))?;
        assert_eq!(settings.application, AppSettings {
            example_name: "example value".into(),
            nested_field: NestedSetting {
                foo: "foo".into(),
                bar: false,
            },
        });
        CustomSettings::override_field(
            &mut settings.application.example_name,
            "/overridden/path/to/cert/key.pem".to_string()
        )?;
        assert_eq!(settings.application, AppSettings {
            example_name: "/overridden/path/to/cert/key.pem".into(),
            nested_field: NestedSetting {
                foo: "foo".into(),
                bar: false,
            },
        });
        Ok(())
    }

}
