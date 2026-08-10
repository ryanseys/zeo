# The power_assert shape: a load-time probe that raises LoadError before any
# definitions, so the file contributes nothing when the probe fails.
raise LoadError, "probe says no"
GOOD = 1
