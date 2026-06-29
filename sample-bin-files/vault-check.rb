#!/usr/bin/env ruby
# frozen_string_literal: true

# Gendaldea vault pre-commit check.
#
# Blocks a commit that introduces, in a *staged* World/ or Books/ bible file:
#   - a dangling wikilink ([[Target]] with no matching file basename or alias), or
#   - malformed frontmatter (no YAML block, or missing/invalid `type`/`status`).
#
# Only staged bible files are inspected, so pre-existing issues elsewhere never
# block an unrelated commit. Run standalone any time:  ruby bin/vault-check.rb
# Bypass intentionally:  git commit --no-verify

VALID_STATUS = %w[idea outline stub draft canon generated drafting revising done].freeze

def repo_root
  `git rev-parse --show-toplevel`.strip
end

def staged_bible_files
  out = `git diff --cached --name-only -z --diff-filter=ACM`
  files = []
  out.split("\0").each do |f|
    next if f.empty? || !f.end_with?(".md")
    next unless f.start_with?("World/") || f.start_with?("Books/")
    next if f.include?("/_builds/") || f.start_with?("Books/_builds/")
    # Longform index is a tool-managed build artifact (no frontmatter by design).
    next if File.basename(f) == "Index.md"

    files << f if File.exist?(f)
  end
  files
end

# Every linkable name in the vault: file basenames + frontmatter aliases.
def all_names
  names = Set.new
  alias_re = /^aliases:\s*\[(.*?)\]/m
  Dir.glob("**/*.md").each do |f|
    names << File.basename(f, ".md").downcase
    begin
      text = File.read(f, encoding: "UTF-8")
    rescue SystemCallError
      next
    end
    m = text.match(/\A---\n(.*?)\n---/m)
    next unless m

    am = m[1].match(alias_re)
    next unless am

    am[1].scan(/"([^"]+)"|'([^']+)'/) do |dq, sq|
      v = (dq || sq).strip
      names << v.downcase unless v.empty?
    end
  end
  names
end

def frontmatter(text)
  m = text.match(/\A---\n(.*?)\n---/m)
  return nil unless m

  d = {}
  m[1].each_line do |line|
    mm = line.match(/\A(\w+):\s*(.*)$/)
    d[mm[1]] = mm[2].strip if mm
  end
  d
end

def main
  require "set"

  root = repo_root
  Dir.chdir(root) unless root.empty?
  staged = staged_bible_files
  return 0 if staged.empty?

  names = all_names
  errors = []
  staged.each do |f|
    text = File.read(f, encoding: "UTF-8")
    fm = frontmatter(text)
    if fm.nil?
      errors << [f, "missing YAML frontmatter block"]
    else
      errors << [f, "frontmatter missing 'type'"] unless fm.key?("type")
      status = fm["status"]
      if status.nil?
        errors << [f, "frontmatter missing 'status'"]
      elsif !VALID_STATUS.include?(status)
        errors << [f, "invalid status '#{status}' (expected #{VALID_STATUS.sort.join('/')})"]
      end
    end
    text.scan(/\[\[([^\]|#\\]+)/) do |target,|
      target = target.strip
      errors << [f, "dangling wikilink [[#{target}]] (no file or alias)"] unless names.include?(target.downcase)
    end
  end

  if errors.any?
    puts "vault-check: FAIL (#{errors.length} issue(s) in staged bible files)"
    errors.each { |f, msg| puts "  #{f}: #{msg}" }
    return 1
  end
  puts "vault-check: OK (#{staged.length} staged bible file(s) clean)"
  0
end

exit(main) if __FILE__ == $PROGRAM_NAME
