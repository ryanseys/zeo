# frozen_string_literal: true

# Stands in for the readline gem's lib/readline.rb. That file probes for the
# C `readline.<DLEXT>` extension at load time and falls back to reline when
# the probe raises. Under zeo no readline extension can ever exist, so the
# probe is replaced by its one possible outcome -- ruby's own arrangement
# since 3.3: Readline IS Reline.
require "reline"
Readline = Reline
