t = Time.utc(2026, 7, 16, 13, 45, 30)
p t.deconstruct_keys(nil)
p t.deconstruct_keys([:zone])
__END__
{year: 2026, month: 7, day: 16, yday: 197, wday: 4, hour: 13, min: 45, sec: 30, subsec: 0, dst: false, zone: "UTC"}
{zone: "UTC"}
