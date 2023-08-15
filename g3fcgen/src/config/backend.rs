/*
 * Copyright 2023 ByteDance and/or its affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::{anyhow, Context};
use openssl::pkey::{PKey, Private};
use openssl::x509::X509;
use yaml_rust::Yaml;

static BACKEND_CONFIG_LOCK: OnceLock<Arc<OpensslBackendConfig>> = OnceLock::new();

pub(crate) fn get_config() -> Option<Arc<OpensslBackendConfig>> {
    BACKEND_CONFIG_LOCK.get().cloned()
}

pub(crate) struct OpensslBackendConfig {
    pub(crate) ca_cert: X509,
    pub(crate) ca_key: PKey<Private>,
}

pub(super) fn load_config(value: &Yaml) -> anyhow::Result<()> {
    if let Yaml::Hash(map) = value {
        let mut ca_cert: Option<X509> = None;
        let mut ca_key: Option<PKey<Private>> = None;
        let lookup_dir = g3_daemon::config::get_lookup_dir(None)?;

        g3_yaml::foreach_kv(map, |k, v| match g3_yaml::key::normalize(k).as_str() {
            "ca_certificate" => {
                let cert = g3_yaml::value::as_openssl_certificates(v, Some(lookup_dir))
                    .context(format!("invalid openssl certificate value for key {k}"))?
                    .pop()
                    .ok_or_else(|| anyhow!("no valid openssl certificate key found"))?;
                ca_cert = Some(cert);
                Ok(())
            }
            "ca_private_key" => {
                let key = g3_yaml::value::as_openssl_private_key(v, Some(lookup_dir))
                    .context(format!("invalid openssl private key value for key {k}"))?;
                ca_key = Some(key);
                Ok(())
            }
            _ => Err(anyhow!("invalid key {k}")),
        })?;

        let Some(ca_cert) = ca_cert else {
            return Err(anyhow!("no ca certificate set"));
        };
        let Some(ca_key) = ca_key else {
            return Err(anyhow!("no ca private key set"));
        };

        BACKEND_CONFIG_LOCK
            .set(Arc::new(OpensslBackendConfig { ca_cert, ca_key }))
            .map_err(|_| anyhow!("duplicate backend config"))?;
        Ok(())
    } else {
        Err(anyhow!(
            "yam value type for the backend config should be 'map'"
        ))
    }
}