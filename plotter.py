from lxml import etree

from plot_frame import PlotFrame, Plot, TextBox
import data_generators as g
import util
#import structures

def score_info(plotter, score):
	datetime = score.findtext("DateTime")
	percent = float(score.findtext("WifeScore"))*100
	percent = round(percent * 100) / 100 # Round to 2 places
	chart = util.find_parent_chart(plotter.xml, score)
	pack, song = chart.get("Pack"), chart.get("Song")
	
	if len(score.findall("SkillsetSSRs")) == 1:
		msd = float(score.findtext(".//Overall"))
		score_value = round(g.score_to_wifescore(score), 2)
		return f'{datetime}    {percent}%    MSD: {msd}    Score: {score_value}    "{pack}" -> "{song}"'
	else:
		util.logger.warning("Selected scatter point doesn't have SkillsetSSRs data")
		return f'{datetime}    {percent}%    "{pack}" -> "{song}"'

def session_info(plotter, data):
	(prev_rating, then_rating, num_scores, length) = data
	prev_rating = round(prev_rating, 2)
	then_rating = round(then_rating, 2)
	length = round(length)
	
	return f'From {prev_rating} to {then_rating}    Total {length} minutes ({num_scores} scores)'

class Plotter:
	def __init__(self, infobar):
		frame = PlotFrame(infobar)
		self.frame = frame
		
		cmap = ['#1f77b4', '#ff7f0e', '#2ca02c', '#d62728',	'#9467bd',
				'#8c564b', '#e377c2', '#7f7f7f', '#bcbd22', '#17becf']
		
		a = TextBox(self, frame, 3)
		b = TextBox(self, frame, 3)
		c = TextBox(self, frame, 6)
		self.frame.next_row()
		
		d = TextBox(self, frame, 6)
		e = TextBox(self, frame, 6)
		self.frame.next_row()
		
		f = Plot(self, frame, 6, flags="time_xaxis", title="Wife score over time")
		f.set_args(cmap[0], click_callback=score_info)
		
		g_ = Plot(self, frame, 6, flags="time_xaxis manip_yaxis", title="Manipulation over time (log scale)")
		g_.set_args(cmap[3], click_callback=score_info)
		self.frame.next_row()
		
		h = Plot(self, frame, 6, flags="time_xaxis accuracy_yaxis", title="Accuracy over time (log scale)")
		h.set_args(cmap[1], click_callback=score_info)
		
		i = Plot(self, frame, 6, flags="time_xaxis", title="Rating improvement per session (x=date, y=session length, bubble size=rating improvement)")
		i.set_args(cmap[2], type_="bubble", click_callback=session_info)
		self.frame.next_row()
		
		m = Plot(self, frame, 6, flags="time_xaxis step", title="Skillsets over time")
		colors = ["ffffff", *util.skillset_colors] # Include overall
		legend = ["Overall", *util.skillsets] # Include overall
		m.set_args(colors, legend=legend, type_="stacked line")
		
		n = Plot(self, frame, 6, title="Distribution of hit offset")
		n.set_args(cmap[6], type_="bar")
		self.frame.next_row()
		
		j = Plot(self, frame, 6, title="Number of plays per hour of day")
		j.set_args(cmap[4], type_="bar")
		
		k = Plot(self, frame, 6, flags="time_xaxis", title="Number of plays each week")
		k.set_args(cmap[5], type_="bar", width=604800*0.8)
		self.frame.next_row()
		
		l = Plot(self, frame, 12, title="Skillsets trained per week")
		l.set_args(util.skillset_colors, legend=util.skillsets, type_="stacked bar")
		self.frame.next_row()
		
		self.plots = [a,b,c,d,e,f,g_,h,i,m,n,j,k,l] #opqrstu...
	
	def draw(self, xml_path, replays_path, qapp):
		print("Opening xml..")
		try: # First try UTF-8
			xmltree = etree.parse(xml_path, etree.XMLParser(encoding='UTF-8'))
		except: # If that doesn't work, fall back to ISO-8859-1
			util.logger.exception("XML parsing with UTF-8 failed")
			xmltree = etree.parse(xml_path, etree.XMLParser(encoding='ISO-8859-1'))
		xml = xmltree.getroot()
		self.xml = xml
		print("Parsing replays..")
		replays = replays_path
		
		print("Generating textboxes..")
		p = iter(self.plots)
		
		next(p).draw(g.gen_textbox_text_2(xml))
		next(p).draw(g.gen_textbox_text_3(xml))
		next(p).draw(g.gen_textbox_text(xml))
		qapp.processEvents()
		
		next(p).draw(g.gen_textbox_text_5(xml, replays))
		next(p).draw(g.gen_textbox_text_4(xml, replays))
		qapp.processEvents()
		
		print("Generating wifescore plot..")
		next(p).draw_with_given_args(g.gen_wifescore(xml))
		qapp.processEvents()
		
		print("Generating manip plot..")
		data = g.gen_manip(xml, replays) if replays else "[please load replay data]"
		next(p).draw_with_given_args(data)
		qapp.processEvents()
		
		print("Generating accuracy plot..")
		next(p).draw_with_given_args(g.gen_accuracy(xml))
		qapp.processEvents()
		
		print("Generating session bubble plot..")
		next(p).draw_with_given_args(g.gen_session_rating_improvement(xml))
		qapp.processEvents()
		
		print("Generating skillset development..")
		next(p).draw_with_given_args(g.gen_skillset_development(xml))
		qapp.processEvents()
		
		print("Generating hit offset distribution..")
		data = g.gen_hit_distribution(xml, replays) if replays else "[please load replay data]"
		next(p).draw_with_given_args(data)
		qapp.processEvents()
		
		print("Generating plays per hour of day..")
		next(p).draw_with_given_args(g.gen_plays_by_hour(xml))
		qapp.processEvents()
		
		print("Generating plays for each week..")
		next(p).draw_with_given_args(g.gen_plays_per_week(xml))
		qapp.processEvents()
		
		print("Generating session skillsets..")
		next(p).draw_with_given_args(g.gen_session_skillsets(xml))
		qapp.processEvents()
		
		print("Done")
