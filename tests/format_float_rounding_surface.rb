# Ruby rounds a float conversion on the SHORTEST round-trip decimal (ties to
# even), not on the exact binary double -- 2.675 is stored as 2.67499999999999982,
# so C printf answers 2.67 where ruby answers 2.68, and 2.345 goes the other way.
# `%e` and `%g` round the same string.
vals = [0.0, -0.0, 1.0, 0.5, 2.5, 3.5, 0.05, 0.005, 0.0005, 2.675, 2.345, 1.005, 8.835,
        0.145, 1.115, 1.255, 0.615, 4.985, 123.456, 1e10, 1e-10, 1.0/3, 2.0/3, 9.995,
        99.995, 0.9999, 1e15, 1e16, 1.23456789012345e5, 0.1, 0.2, 0.3, 1e-5, 12345.6789,
        -2.675, -0.005, 1e100, 1e-100, 3.14159265358979, 1e300]
convs = ["%f","%.0f","%.1f","%.2f","%.3f","%.5f","%.10f","%.17f","%.20f","%e","%.0e","%.1e","%.2e","%.5e","%E","%g","%.1g","%.2g","%.3g","%.6g","%.10g","%G","%10.2f","%-10.2f","%+.2f","%08.2f","% .2f","%#.3g"]
convs.each do |c|
  puts(vals.map { |v| format(c, v) }.join("|"))
end
