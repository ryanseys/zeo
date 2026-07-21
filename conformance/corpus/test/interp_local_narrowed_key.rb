module Db
  def self.prepare(sql)
  end
  def self.column_value(stmt, i)
  end
  def self.column_count(stmt)
  end
  def self.column_name(stmt, i)
  end
end
module ActiveRecord
  class << self
    attr_accessor :adapter
    def self.find_by!(conditions)
      ActiveRecord.adapter.where(table_name, conditions.to_h).map { |row| instantiate(row) }
    end
  end
end
module ActiveRecord
  class Relation
    def where(condition)
    end
    def to_a
      rows = ActiveRecord.adapter.select_rows(to_sql)
    end
    def map
      to_a.map { |x| yield x }
    end
    def update_all(updates)
      set_sql = if updates.is_a?(Hash)
      end
    end
    def to_sql
    end
  end
end
module SqliteAdapter
  def self.where(table, conditions)
    sql = "SELECT * FROM #{table}"
    sql += " WHERE #{build_where(conditions)}" unless conditions.empty?
    sql
  end
  def self.select_rows(sql)
    stmt = Db.prepare(sql)
    rows = []
    ncols = Db.column_count(stmt)
    names = []
    i = 0
    while i < ncols
      names.push(Db.column_name(stmt, i))
      row = {}
      while i < ncols
        row[names[i]] = Db.column_value(stmt, i)
      end
      rows << row
    end
    rows
  end
  def self.build_where(conditions)
    conditions.map { |k, v| "#{k} = #{escape_value(v)}" }.join(" AND ")
  end
  def self.escape_value(v)
  end
end
ActiveRecord.adapter = SqliteAdapter
class QueryCountTest
  def test_articles_index_is_two_queries_not_n_plus_one
    per_article = {}
    unless per_article.empty?
    end
  end
  def test_rechaining_a_loaded_relation_requeries
    rel = ActiveRecord::Relation.new
    ids = [1]
    rel.where("id = #{ids[0]}")
  end
end
QueryCountTest.new.test_rechaining_a_loaded_relation_requeries
puts "ok"
