p(require("rbconfig"))
f = "rbconfig"
p(require(f))
p RbConfig::CONFIG.key?("host_os")
