def rect(width:, height:) = width * height
configs = [{ width: 2, height: 3 }, { width: 5, height: 5 }]
p configs.map { |c| rect(**c) }
