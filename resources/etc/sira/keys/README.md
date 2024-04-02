# SSH keys

**action** and **manifest**: properly formed, sample SSH key pairs for use in testing.

**unreadable** and **unreadable.pub**: fake SSH key files with no permissions, used for generating error messages in `ssh-keygen`. These files are automatically created and deleted by the relevant tests and should not be committed to source control, especially since Git won't be able to read them!

**does_not_exist** and **doesnotexist**: reserved for tests that need to provide a key path that does not exist. No files with these names should be created.

# Permissions

The production keys have `root:<sira-user> 0640` permissions by default. The keys here, however, must be owned by the local owner of the Git repository. When you try to use a key that you own, OpenSSH rejects `0640` permissions as too open. Therefore, you must change these files `0600`.
