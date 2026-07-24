# %:::z collapses the UTC offset to the minimal precision that loses nothing
# (#2713); a fixed-offset time (Time.at in:) reports its own offset.
t = Time.utc(2026, 7, 16, 13, 45, 30)
p t.strftime("%:z")
p t.strftime("%::z")
p t.strftime("%:::z")
p Time.at(0, in: "+05:30").strftime("%:::z")
p Time.at(0, in: "+05:00").strftime("%:::z")
p Time.at(0, in: "-08:00").strftime("%:::z")
# bare %z comes from the receiver's own offset, never C strftime's tm_gmtoff
# (platform-varying; macOS reported the machine-local zone for a UTC receiver)
p t.strftime("%z")
p Time.at(0, in: "+05:30").strftime("%z")
p Time.at(0, in: "-08:00").strftime("%z")
