       identification division.
       program-id. tranrept-sort-reference.
       environment division.
       input-output section.
       file-control.
           select source-file assign to "SORTIN"
               organization is sequential.
           select result-file assign to "SORTOUT"
               organization is sequential.
           select work-file assign to "SORTWORK".
       data division.
       file section.
       fd source-file.
       01 source-record pic x(350).
       fd result-file.
       01 result-record pic x(350).
       sd work-file.
       01 work-record.
          05 work-data.
             10 filler pic x(262).
             10 card-number pic x(16).
             10 filler pic x(72).
          05 input-position pic 9(9).
       working-storage section.
       01 next-position pic 9(9) value zero.
       01 end-input pic 9 value zero.
       01 end-sort pic 9 value zero.
       procedure division.
           sort work-file on ascending key card-number
               input-position
               input procedure filter-input
               output procedure write-output
           goback.
       filter-input.
           open input source-file
           perform until end-input = 1
               read source-file
                   at end move 1 to end-input
                   not at end
                       if source-record(305:10) >= "2022-01-01"
                           and source-record(305:10) <= "2022-07-06"
                           add 1 to next-position
                           move source-record to work-data
                           move next-position to input-position
                           release work-record
                       end-if
               end-read
           end-perform
           close source-file.
       write-output.
           open output result-file
           perform until end-sort = 1
               return work-file
                   at end move 1 to end-sort
                   not at end
                       move work-data to result-record
                       write result-record
               end-return
           end-perform
           close result-file.
