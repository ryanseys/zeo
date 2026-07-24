# Proc#source_location returns [file, line] for a proc literal. Assert the
# structure and types (the file path is invocation-dependent, and the exact
# line requires the line map, which the test harness disables). Regression #2649.
loc = ->(x) { x }.source_location
p loc.class
p loc.length
p loc[0].class
p loc[1].class
