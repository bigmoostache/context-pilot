Currently: one .context-pilot/state.json. This is ok for a single-worker setup, but is not if we want multi workers/ agents working together. here is what I want instead:

- a single config.json for (put a bit or order and structuring, I give things en vrac)
    * reload_requested
    * active_theme
    * owner_pid
    * dev_mode
    * llm_provider
    * anthropic_model
    * grok_model
    * groq_model
    * memories (memories should be shared)
    * next_memory_id (memories should be shared)
    * systems (systems should be shared)
    * next_system_id (systems should be shared)
    * draft_input
    * draft_cursor
    * tree_filter (should be shared)
    * tree_descriptions (should be shared)
    * cleaning_threshold
    * cleaning_target_proportion
    * context_budget
    * selected_context
    * global_next_uid
- then, a states folder with, for now, only a main_worker.json
    * panel_uid_to_local_panel_id_map
    * important_panel_uuids (same than above, but store directly ids of chat, tree, wip, memories, world, changes and scratch)
    * next_tool_id
    * next_result_id
    * next_todo_id
    * todos
    * context
    * disabled_tools
    * git_show_diffs
    * tree_open_folders
    * active_system_id
    * scratchpad_cells
    * next_scratchpad_id
    
- then, a panels folder, 100% shared. 
    -> For conversation panels:
        * message_uids 

Remarks:
- I do not need backwards compatibility
- uids should be noted UID_1_{letter here}, UID_2_{letter here}, etc etc. the letter being the twpe of the element. but the iterator is the same for everyone.
- uids should never be shown, either to ui or to llms. they are here for shared elements, but all shared elements should have a remap to a local reindex that restarts at 1: U messages, A messages, T messages, R messages, P panels
- since everyone sees the exact same memory panel, no need for a remap here
- same for seed/ system prompts
- scratch pad is 100% local, as well as TODO and tree state so no need for uids or remap here!