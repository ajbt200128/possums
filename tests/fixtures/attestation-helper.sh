#!/bin/sh
# Interpreting the body avoids ETXTBSY from inherited writable fixture descriptors.
exec /bin/sh "$2"
