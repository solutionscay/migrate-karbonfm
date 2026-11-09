-- V4: the switches of the optional integrations (I6; AP-12, BR-INTEG-02). Both start off, on a new install and on a
-- migrated one: nothing is called until the user turns one on in Settings and saves its key (keys stay in the
-- keychain, AP-03). The source chose the address service per session (Picker::AddressServiceProvider, a global); the
-- target keeps the choice per install.
ALTER TABLE system_setting ADD COLUMN address_suggestion_provider TEXT; -- new: NULL = off; 'Google Places' or 'SmartyStreets' (Picker::AddressServiceProvider)
ALTER TABLE system_setting ADD COLUMN fullcontact_enabled_ind NUMERIC; -- new: 1 = FullContact person enrichment on; NULL or 0 = off
