-- exports a Numbers spreadsheet: osascript - <in> <out> <pdf|xlsx|csv>
on run argv
	set inFile to POSIX file (item 1 of argv)
	set outFile to POSIX file (item 2 of argv)
	set fmt to item 3 of argv
	set wasRunning to application "Numbers" is running
	tell application "Numbers"
		set doc to open inFile
		if fmt is "pdf" then
			export doc to outFile as PDF
		else if fmt is "xlsx" then
			export doc to outFile as Microsoft Excel
		else
			export doc to outFile as CSV
		end if
		close doc saving no
		if not wasRunning then quit
	end tell
end run
