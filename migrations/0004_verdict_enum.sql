-- Constrain verdict at the write. The read side parses fail-loud
-- (repo::parse_verdict), but with VARCHAR a corrupt write only surfaces when
-- it poisons reads of that whole report; as an ENUM the bad write itself
-- errors, where the producer's spool retry makes the failure visible. All
-- stored values already come from Verdict::to_string, so this rewrites nothing.
ALTER TABLE check_result
    MODIFY verdict ENUM('pass', 'warn', 'fail', 'skip') NOT NULL;
