# /resources/etc/

In a production environment, Sira's configuration files live in `/etc`. Obviously unit tests shouldn't depend on the contents of the test machine's actual `/etc`, so unit tests refer to this directory instead. *There is no security impact from revealing private SSH keys in this directory subtree, as they are only for unit tests.*
