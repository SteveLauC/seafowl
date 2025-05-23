use object_store::{
    gcp::GoogleCloudStorageBuilder, gcp::GoogleConfigKey, path::Path, ObjectStore,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct GCSConfig {
    pub bucket: String,
    pub prefix: Option<String>,
    pub google_application_credentials: Option<String>,
}

impl GCSConfig {
    pub fn from_hashmap(
        map: &HashMap<String, String>,
    ) -> Result<Self, object_store::Error> {
        Ok(Self {
            bucket: map.get("bucket").unwrap().clone(),
            prefix: map.get("prefix").map(|s| s.to_string()),
            google_application_credentials: map
                .get("google_application_credentials")
                .map(|s| s.to_string()),
        })
    }

    pub fn from_bucket_and_options(
        bucket: String,
        map: &mut HashMap<String, String>,
    ) -> Result<Self, object_store::Error> {
        Ok(Self {
            bucket,
            prefix: None,
            google_application_credentials: map
                .remove("format.google_application_credentials"),
        })
    }

    pub fn to_hashmap(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert(
            GoogleConfigKey::Bucket.as_ref().to_string(),
            self.bucket.clone(),
        );
        if let Some(prefix) = &self.prefix {
            map.insert("prefix".to_string(), prefix.clone());
        }
        if let Some(google_application_credentials) = &self.google_application_credentials
        {
            map.insert(
                GoogleConfigKey::ApplicationCredentials.as_ref().to_string(),
                google_application_credentials.clone(),
            );
        }
        map
    }

    pub fn bucket_to_url(&self) -> String {
        format!("gs://{}", &self.bucket)
    }

    pub fn build_google_cloud_storage(
        &self,
    ) -> Result<Arc<dyn ObjectStore>, object_store::Error> {
        let mut builder: GoogleCloudStorageBuilder =
            GoogleCloudStorageBuilder::new().with_bucket_name(self.bucket.clone());

        builder = if let Some(path) = &self.google_application_credentials {
            builder.with_service_account_path(path.clone())
        } else {
            builder
        };

        let store = builder.build()?;
        Ok(Arc::new(store))
    }

    pub fn get_base_url(&self) -> Option<Path> {
        self.prefix
            .as_ref()
            .map(|prefix| Path::from(prefix.as_ref()))
    }
}

pub fn map_options_into_google_config_keys(
    input_options: HashMap<String, String>,
) -> Result<HashMap<GoogleConfigKey, String>, object_store::Error> {
    let mut mapped_keys = HashMap::new();

    for (key, value) in input_options {
        match GoogleConfigKey::from_str(&key) {
            Ok(config_key) => {
                mapped_keys.insert(config_key, value);
            }
            Err(err) => {
                return Err(err);
            }
        }
    }

    Ok(mapped_keys)
}

pub fn add_google_cloud_storage_environment_variables(
    options: &mut HashMap<GoogleConfigKey, String>,
) {
    // Insert the service account {path, key} from env only if neither the path nor the key are
    // already supplied.
    //
    // This is because if the same config was already passed we don't want to override it, and if
    // the other config was passed the client builder will error out (doesn't support both file and
    // key).
    if !options.contains_key(&GoogleConfigKey::ServiceAccount)
        && !options.contains_key(&GoogleConfigKey::ServiceAccountKey)
    {
        if let Ok(service_account_path) = env::var("GOOGLE_SERVICE_ACCOUNT") {
            options.insert(GoogleConfigKey::ServiceAccount, service_account_path);
        } else if let Ok(service_account_path) = env::var("GOOGLE_SERVICE_ACCOUNT_KEY") {
            options.insert(GoogleConfigKey::ServiceAccountKey, service_account_path);
        }
    }

    // Override all other configs from env vars
    for (os_key, os_value) in env::vars_os() {
        if let (Some(key), Some(value)) = (os_key.to_str(), os_value.to_str()) {
            if key.starts_with("GOOGLE_") {
                if let Ok(config_key) = key.to_ascii_lowercase().parse() {
                    if !matches!(
                        config_key,
                        GoogleConfigKey::ServiceAccount
                            | GoogleConfigKey::ServiceAccountKey
                    ) {
                        options.entry(config_key).or_insert(value.to_string());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_from_hashmap_with_all_fields() {
        let mut map = HashMap::new();
        map.insert("bucket".to_string(), "my-bucket".to_string());
        map.insert("prefix".to_string(), "my-prefix".to_string());
        map.insert(
            "google_application_credentials".to_string(),
            "/path/to/credentials.json".to_string(),
        );

        let config =
            GCSConfig::from_hashmap(&map).expect("Failed to create config from hashmap");
        assert_eq!(config.bucket, "my-bucket");
        assert_eq!(config.prefix, Some("my-prefix".to_string()));
        assert_eq!(
            config.google_application_credentials,
            Some("/path/to/credentials.json".to_string())
        );
    }

    #[test]
    fn test_config_from_hashmap_with_missing_optional_fields() {
        let mut map = HashMap::new();
        map.insert("bucket".to_string(), "my-bucket".to_string());

        let config =
            GCSConfig::from_hashmap(&map).expect("Failed to create config from hashmap");
        assert_eq!(config.bucket, "my-bucket");
        assert!(config.prefix.is_none());
        assert!(config.google_application_credentials.is_none());
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn test_config_from_hashmap_without_bucket() {
        let map = HashMap::new();
        GCSConfig::from_hashmap(&map).unwrap();
    }

    #[test]
    fn test_build_google_cloud_storage_from_config_with_all_fields() {
        // Create a temporary file for the credentials
        let temp_file = NamedTempFile::new().expect("Failed to create temporary file");

        // Write some mock credentials to the file
        let credentials_content = r#"{
            "type": "service_account",
            "project_id": "gcloud-123456",
            "private_key_id": "123456",
            "private_key": "-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCxjzFuu7kO+sfY\nXPq0EZo1Oth9YjCyrhIQr6XavJQyD/OT9gcd9Q5+/VvLwCXBijEgVdXFQf5Tcsh2\ndpp/hOjGuc7Lh9Kk+DtebUDZ9AIF92LvRX2yKJJ4a6zqV9iEqCfxAhSrwsYMLnp0\nGbxG0ACUR/VdLv8U2ctNDG4DL8jk6yYowABbsL/074GOFWtwW99w1BJb09+l0f2l\njIom15iY897W1gjOBskM7fsHm3WwlCwD/+4PPodp8PRIjvefnMwx7E0Lu6IcJ8Kg\n4Rhm1Rk5hJWKWEgQHmZ4ik4kc/FKdHRMGERkMY5VVYoZ6bUx7OdhF7Vt3HVZDA88\nsx9fbTBxAgMBAAECggEAAWSAHMA4KVfqLVY9WSAyN2yougMFIsGevqbCBD8qYmIh\npO1vDNsZLAHMsIJnSWdOD1TdAlkMJ5dk3xj7CTj/ol9esdX03vpbbNgqhAsX4PgZ\nvIqs+7K5w1wE1SmvNwsilQ9RHi++4eWTbEmvYlbLSl5uHDb8JSu4HniUfE3po3H5\nWDj01OMSe9dhaXrzhqOn2qo37XJ9xF1VCSkY3JRj3cY7W7crVE3UmDyYT+ZE1Tei\nyYhrZh1QDFeQVCFiHEP3RA1T/MYaFn1ylkwGcvgFvoB81vOJaVEXh1Xldwx/6KZC\nyrXBlnVqa//IuCtEE4zTl146G99kRdQFrAdqTadlSQKBgQDauQefH+zCpxTaO03E\nlzGoXr9mxo6Rzhim60e+uDgkCnDhElc3rqiuxFH6QNORa2/A/zvc7iHYZsu8QAvB\n776S9rrpxHoc1271fLqzMBR6gDkTzh/MjUJnsPNjnfehE2h6U8Zoeq755Xv9S85I\nuk9bIJzs5JH6xBEDxnIb/ier5wKBgQDP0i9jTb5TgrcqYYpjURsHGQRv+6lOaZrC\nD94vNDmhTLg3kW5b2BD0ZeZwGCwiSOSqL/5fjlRie94pPnIn6pm5uGgndgdRLQvw\nIdpRyvAUAOY7SnoLhZjVue4syzwV3k7+d4x7LrzpZclBH8uc3sLU3vOSsmFRIkf+\nfK9qcVv15wKBgQDL2fHRi/algQW9U9JqbKQakZwAVQThvd1aDSVECvxAEv8btnVV\nb1LF+DGTdUH6YdC5ZujLQ6KFx2ERZfvPV/wdixmv8LADG4LOB98WTLR5a/JGlDEs\n+2ctr01YxgzasnUItfXQwK8+N3U1Iab0P7jgbOf1Hh80QfK9uwH1Nw6QdwKBgCuP\nigFNpWxJxOzsPx6sPHcTZlu2q3lVJ2wv+Ul5r+7AbwiuwiwcMQmZZmDuoCmbj9qg\nbrhG1CdEgX+xqCn3wbstDR/gXI5GW+88mU91szbuLVQWO1i46x05eNQI0ZJf47zx\nABA97rkZbcLp0DsUclA+X13LaByii+aq6fXsxvLXAoGBALzkBzJ/SOvotz/UnBxl\nGU9QWmptZttaqtLKizPNQZpY1KO9VxeyoGbkTnN0M58ktpIp8LGlSJejk/tkRKBG\nUFRW/v49GW3eCgl4D+MOTFLCJDT68D2lp4F9hdBHsoH17ZdHy8rennmJN3QExIjx\n0xoq6OYjjzNwhFqkPl0H6HrM\n-----END PRIVATE KEY-----\n",
            "client_email": "mail@gcloud-123456.iam.gserviceaccount.com",
            "client_id": "102784232161964177687",
            "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            "token_uri": "https://oauth2.googleapis.com/token",
            "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
            "client_x509_cert_url": "https://www.googleapis.com/robot/v1/metadata/x509/gcloud-acc%40gcloud-123456.iam.gserviceaccount.com"
            }"#;

        fs::write(temp_file.path(), credentials_content)
            .expect("Failed to write to temporary file");

        let result = GCSConfig {
            bucket: "my-bucket".to_string(),
            prefix: Some("my-prefix".to_string()),
            google_application_credentials: Some(
                temp_file.path().to_str().unwrap().to_string(),
            ),
        }
        .build_google_cloud_storage();

        assert!(result.is_ok(), "Expected Ok, got Err: {result:?}");

        let store = result.unwrap();
        let debug_output = format!("{store:?}");

        assert!(debug_output.contains("bucket_name: \"my-bucket\""));
    }

    #[test]
    fn test_build_google_cloud_storage_from_config_with_missing_optional_fields() {
        let result = GCSConfig {
            bucket: "my-bucket".to_string(),
            prefix: None,
            google_application_credentials: None,
        }
        .build_google_cloud_storage();

        assert!(result.is_ok(), "Expected Ok, got Err: {result:?}");

        let store = result.unwrap();
        let debug_output = format!("{store:?}");

        assert!(debug_output.contains("bucket_name: \"my-bucket\""));
    }

    #[test]
    fn test_map_options_into_google_config_keys_with_valid_keys() {
        let mut input_options = HashMap::new();
        input_options.insert(
            "google_service_account".to_string(),
            "my_google_service_account".to_string(),
        );
        input_options.insert("bucket".to_string(), "my-bucket".to_string());
        input_options.insert(
            "google_service_account_key".to_string(),
            "my_google_service_account_key".to_string(),
        );

        let result = map_options_into_google_config_keys(input_options);
        assert!(result.is_ok());

        let mapped_keys = result.unwrap();
        assert_eq!(
            mapped_keys.get(&GoogleConfigKey::ServiceAccount),
            Some(&"my_google_service_account".to_string())
        );
        assert_eq!(
            mapped_keys.get(&GoogleConfigKey::Bucket),
            Some(&"my-bucket".to_string())
        );
        assert_eq!(
            mapped_keys.get(&GoogleConfigKey::ServiceAccountKey),
            Some(&"my_google_service_account_key".to_string())
        );
    }

    #[test]
    fn test_map_options_into_google_config_keys_with_invalid_key() {
        let mut input_options = HashMap::new();
        input_options.insert("invalid_key".to_string(), "some_value".to_string());

        let result = map_options_into_google_config_keys(input_options);
        assert!(result.is_err());

        let error = result.err().unwrap();
        print!("ERROR1: {:?}", error.to_string());
        assert_eq!(
            error.to_string(),
            "Configuration key: 'invalid_key' is not valid for store 'GCS'."
        )
    }

    #[test]
    fn test_map_options_into_google_config_keys_with_mixed_keys() {
        let mut input_options = HashMap::new();
        input_options.insert("invalid_key".to_string(), "some_value".to_string());
        input_options.insert("bucket".to_string(), "my-bucket".to_string());

        let result = map_options_into_google_config_keys(input_options);
        assert!(result.is_err());

        let error = result.err().unwrap();
        assert_eq!(
            error.to_string(),
            "Configuration key: 'invalid_key' is not valid for store 'GCS'."
        )
    }

    #[test]
    fn test_map_options_into_google_config_keys_empty_input() {
        let input_options = HashMap::new();
        let result = map_options_into_google_config_keys(input_options);
        assert!(result.is_ok());

        let mapped_keys = result.unwrap();
        assert!(mapped_keys.is_empty());
    }

    #[test]
    fn test_get_base_url_with_prefix() {
        let gcs_config = GCSConfig {
            bucket: "my_bucket".to_string(),
            prefix: Some("my_prefix".to_string()),
            google_application_credentials: Some("path/to/credentials.json".to_string()),
        };

        let base_url = gcs_config.get_base_url();
        assert!(base_url.is_some());
        assert_eq!(base_url.unwrap(), Path::from("my_prefix"));
    }

    #[test]
    fn test_get_base_url_without_prefix() {
        let gcs_config = GCSConfig {
            bucket: "my_bucket".to_string(),
            prefix: None,
            google_application_credentials: Some("path/to/credentials.json".to_string()),
        };

        let base_url = gcs_config.get_base_url();
        assert!(base_url.is_none());
    }

    #[test]
    fn test_get_base_url_with_empty_prefix() {
        let gcs_config = GCSConfig {
            bucket: "my_bucket".to_string(),
            prefix: Some("".to_string()),
            google_application_credentials: Some("path/to/credentials.json".to_string()),
        };

        let base_url = gcs_config.get_base_url();
        assert!(base_url.is_some());
        assert_eq!(base_url.unwrap(), Path::from(""));
    }

    #[test]
    fn test_to_hashmap() {
        let gcs_config = GCSConfig {
            bucket: "my_bucket".to_string(),
            prefix: Some("my_prefix".to_string()),
            google_application_credentials: Some("path/to/credentials.json".to_string()),
        };

        let hashmap = gcs_config.to_hashmap();

        assert_eq!(
            hashmap.get(GoogleConfigKey::Bucket.as_ref()),
            Some(&"my_bucket".to_string())
        );
        assert_eq!(hashmap.get("prefix"), Some(&"my_prefix".to_string()));
        assert_eq!(
            hashmap.get(GoogleConfigKey::ApplicationCredentials.as_ref()),
            Some(&"path/to/credentials.json".to_string())
        );
    }

    #[test]
    fn test_to_hashmap_with_none_fields() {
        let gcs_config = GCSConfig {
            bucket: "my_bucket".to_string(),
            prefix: None,
            google_application_credentials: None,
        };

        let hashmap = gcs_config.to_hashmap();

        assert_eq!(
            hashmap.get(GoogleConfigKey::Bucket.as_ref()),
            Some(&"my_bucket".to_string())
        );
        assert_eq!(hashmap.get("prefix"), None);
        assert_eq!(
            hashmap.get(GoogleConfigKey::ApplicationCredentials.as_ref()),
            None
        );
    }

    #[test]
    fn test_bucket_to_url() {
        let config = GCSConfig {
            bucket: "my_bucket".to_string(),
            prefix: Some("my_prefix".to_string()),
            google_application_credentials: Some("path/to/credentials.json".to_string()),
        };

        let url = config.bucket_to_url();
        assert_eq!(url, "gs://my_bucket");
    }
}
