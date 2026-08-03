#[cfg(target_os = "macos")]
mod implementation {
    use super::super::{KeychainBackend, KeychainBackendError};
    use core_foundation::base::TCFType;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::{CFString, CFStringRef};
    use next_infra_core::SecretValue;
    use objc2::rc::Retained;
    use objc2_local_authentication::LAContext;
    use security_framework::base::Error;
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
    use security_framework_sys::access_control::kSecAttrAccessibleWhenUnlockedThisDeviceOnly;
    use security_framework_sys::base::{
        errSecAuthFailed as ERR_SEC_AUTH_FAILED, errSecItemNotFound as ERR_SEC_ITEM_NOT_FOUND,
    };
    use security_framework_sys::item::kSecValueData;
    use security_framework_sys::keychain_item::SecItemAdd;

    const ERR_SEC_MISSING_ENTITLEMENT: i32 = -34_018;
    const ERR_SEC_NOT_AVAILABLE: i32 = -25_291;
    const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25_308;
    const ITEM_LABEL: &str = "Next Infra provider credential";

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecAttrAccessible: CFStringRef;
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct MacosDataProtectionKeychainBackend;

    impl KeychainBackend for MacosDataProtectionKeychainBackend {
        fn add(
            &mut self,
            access_group: &str,
            service: &str,
            account: &str,
            value: &[u8],
        ) -> Result<(), KeychainBackendError> {
            let mut password = security_framework::passwords::PasswordOptions::new_generic_password(
                service, account,
            );
            password.use_protected_keychain();
            password.set_access_group(access_group);
            password.set_access_synchronized(Some(false));
            password.set_label(ITEM_LABEL);

            unsafe {
                #[allow(deprecated)]
                password.query.push((
                    CFString::wrap_under_get_rule(kSecAttrAccessible),
                    CFString::wrap_under_get_rule(kSecAttrAccessibleWhenUnlockedThisDeviceOnly)
                        .into_CFType(),
                ));
                #[allow(deprecated)]
                password.query.push((
                    CFString::wrap_under_get_rule(kSecValueData),
                    core_foundation::data::CFData::from_buffer(value).into_CFType(),
                ));
            }
            #[allow(deprecated)]
            let query = CFDictionary::from_CFType_pairs(&password.query);
            let status = unsafe { SecItemAdd(query.as_concrete_TypeRef(), std::ptr::null_mut()) };
            if status == 0 {
                Ok(())
            } else {
                Err(map_error(Error::from_code(status)))
            }
        }

        fn read_no_ui(
            &self,
            access_group: &str,
            service: &str,
            account: &str,
        ) -> Result<SecretValue, KeychainBackendError> {
            let context = unsafe { LAContext::new() };
            unsafe { context.setInteractionNotAllowed(true) };
            let context = Retained::into_raw(context).cast();

            let mut query = ItemSearchOptions::new();
            query
                .class(ItemClass::generic_password())
                .ignore_legacy_keychains()
                .cloud_sync(Some(false))
                .access_group(access_group)
                .service(service)
                .account(account)
                .load_data(true)
                .limit(1);
            #[allow(deprecated)]
            unsafe {
                query.authentication_context(context);
            }

            match query.search().map_err(map_error)?.into_iter().next() {
                Some(SearchResult::Data(value)) => Ok(SecretValue::new(value)),
                Some(_) => Err(KeychainBackendError::Other),
                None => Err(KeychainBackendError::Missing),
            }
        }

        fn delete(
            &mut self,
            access_group: &str,
            service: &str,
            account: &str,
        ) -> Result<(), KeychainBackendError> {
            let mut query = ItemSearchOptions::new();
            query
                .class(ItemClass::generic_password())
                .ignore_legacy_keychains()
                .cloud_sync(Some(false))
                .access_group(access_group)
                .service(service)
                .account(account);
            query.delete().map_err(map_error)
        }
    }

    fn map_error(error: Error) -> KeychainBackendError {
        match error.code() {
            ERR_SEC_ITEM_NOT_FOUND => KeychainBackendError::Missing,
            ERR_SEC_AUTH_FAILED | ERR_SEC_NOT_AVAILABLE | ERR_SEC_INTERACTION_NOT_ALLOWED => {
                KeychainBackendError::Locked
            }
            ERR_SEC_MISSING_ENTITLEMENT => KeychainBackendError::SigningConfigurationInvalid,
            _ => KeychainBackendError::Other,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn maps_noninteractive_and_unavailable_statuses_without_exposing_context() {
            for code in [
                ERR_SEC_AUTH_FAILED,
                ERR_SEC_NOT_AVAILABLE,
                ERR_SEC_INTERACTION_NOT_ALLOWED,
            ] {
                assert_eq!(
                    map_error(Error::from_code(code)),
                    KeychainBackendError::Locked
                );
            }
        }

        #[test]
        fn maps_missing_item_and_entitlement_failures_separately() {
            assert_eq!(
                map_error(Error::from_code(ERR_SEC_ITEM_NOT_FOUND)),
                KeychainBackendError::Missing
            );
            assert_eq!(
                map_error(Error::from_code(ERR_SEC_MISSING_ENTITLEMENT)),
                KeychainBackendError::SigningConfigurationInvalid
            );
        }
    }
}

#[cfg(target_os = "macos")]
pub use implementation::MacosDataProtectionKeychainBackend;

#[cfg(not(target_os = "macos"))]
#[derive(Clone, Copy, Debug, Default)]
pub struct MacosDataProtectionKeychainBackend;

#[cfg(not(target_os = "macos"))]
impl super::KeychainBackend for MacosDataProtectionKeychainBackend {
    fn add(
        &mut self,
        _access_group: &str,
        _service: &str,
        _account: &str,
        _value: &[u8],
    ) -> Result<(), super::KeychainBackendError> {
        Err(super::KeychainBackendError::Other)
    }

    fn read_no_ui(
        &self,
        _access_group: &str,
        _service: &str,
        _account: &str,
    ) -> Result<next_infra_core::SecretValue, super::KeychainBackendError> {
        Err(super::KeychainBackendError::Other)
    }

    fn delete(
        &mut self,
        _access_group: &str,
        _service: &str,
        _account: &str,
    ) -> Result<(), super::KeychainBackendError> {
        Err(super::KeychainBackendError::Other)
    }
}
