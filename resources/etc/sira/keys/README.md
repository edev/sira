# SSH keys

**action** and **manifest**: properly formed, sample SSH key pairs for use in testing.

**unreadable**: a private SSH key with no permissions, used for generating error messages in `ssh-keygen`. No public key is needed, as this should never be allowed to sign a file. This file is automatically created and deleted by the relevant test and should not be committed to source control, especially since Git won't be able to read it!

**does_not_exist** and **doesnotexist**: reserved for tests that need to provide a key path that does not exist. No files with these names should be created.
